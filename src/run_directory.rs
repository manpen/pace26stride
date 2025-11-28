use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CreateInstanceDirError {
    #[error("Io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No valid instance name passed")]
    EmptyInstanceName,
}

pub struct RunDirectory {
    path: PathBuf,
}

const LOG_PARENT_DIR: &str = "stride-logs";
const LOG_LATEST_LINK: &str = "latest";

const RUN_DIR_FORMAT_SHORT: &str = "run_%y%m%d_%H%M%S"; // used only for first attempt
const RUN_DIR_FORMAT_LONG: &str = "run_%y%m%d_%H%M%S%.6f";

impl RunDirectory {
    pub fn new() -> Result<Self, std::io::Error> {
        Self::new_within(Path::new(LOG_PARENT_DIR))
    }

    pub fn new_within(parent: &Path) -> Result<Self, std::io::Error> {
        std::fs::create_dir_all(parent)?;

        // we create a uniquely timestamped run directory
        let mut format = RUN_DIR_FORMAT_SHORT;
        let path = loop {
            let prefix: String = chrono::Local::now().format(format).to_string();
            let path = parent.join(prefix);
            format = RUN_DIR_FORMAT_LONG;

            // try to create the timestamped directory; if it already exists, retry
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        };

        // now, create or update the "latest" symlink to point to the new log directory
        let latest_path = parent.join(LOG_LATEST_LINK);
        loop {
            match std::os::unix::fs::symlink(path.file_name().unwrap(), &latest_path) {
                Ok(()) => break,

                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}

                Err(e) => return Err(e),
            }

            // if the symlink already existed, only replace it if the symlink target is older
            // (i.e., avoid races here)
            let old_target = latest_path.read_link()?;
            if old_target.file_name() < path.file_name() {
                std::fs::remove_file(&latest_path)?;
            } else {
                break;
            }
        }

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create a subdirectory for the given instance name.
    /// If the directory already exists, appends a suffix to make it unique.
    pub fn create_instance_dir(
        &self,
        instance_name: &str,
    ) -> Result<PathBuf, CreateInstanceDirError> {
        if instance_name.is_empty() {
            return Err(CreateInstanceDirError::EmptyInstanceName);
        }

        for attempt in 0.. {
            let dir = if attempt == 0 {
                self.path.join(instance_name)
            } else {
                self.path.join(format!("{}_{}", instance_name, attempt))
            };

            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(dir),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(CreateInstanceDirError::Io(e)),
            }
        }

        unreachable!()
    }

    pub fn create_instance_dir_for_path(
        &self,
        instance_path: &Path,
    ) -> Result<PathBuf, CreateInstanceDirError> {
        let instance_name = match instance_path.file_stem() {
            Some(x) => x.to_string_lossy(),
            None => return Err(CreateInstanceDirError::EmptyInstanceName),
        };
        self.create_instance_dir(&instance_name)
    }
}

#[cfg(test)]
mod tests {
    use tempdir::TempDir;

    use super::*;

    #[test]
    fn test_log_directory_creation() {
        let parent_dir = TempDir::new("logdir_test").unwrap();
        let parent = parent_dir.path();

        // first run
        {
            let log_dir = RunDirectory::new_within(parent).unwrap();
            assert!(log_dir.path().exists());

            std::fs::write(log_dir.path().join("test"), "test").unwrap();

            assert!(parent.join(LOG_LATEST_LINK).exists());
            let link_target = parent.join(LOG_LATEST_LINK).read_link().unwrap();
            assert_eq!(link_target, log_dir.path().file_name().unwrap());
        }

        // second run
        {
            let log_dir = RunDirectory::new_within(parent).unwrap();
            assert!(log_dir.path().exists());

            std::fs::write(log_dir.path().join("test"), "test").unwrap();

            assert!(parent.join(LOG_LATEST_LINK).exists());
            let link_target = parent.join(LOG_LATEST_LINK).read_link().unwrap();

            assert_eq!(link_target, log_dir.path().file_name().unwrap());
        }
    }

    #[test]
    fn test_instance_dir_creation() {
        let parent_dir = TempDir::new("logdir_test").unwrap();
        let log_dir = RunDirectory::new_within(parent_dir.path()).unwrap();
        let instance_name = "instance1";

        // first job
        let dir1 = log_dir.create_instance_dir(instance_name).unwrap();
        assert!(dir1.exists());

        // second job with same instance name
        let dir2 = log_dir.create_instance_dir(instance_name).unwrap();
        assert!(dir2.exists());

        // make sure the two directories are different
        assert!(dir1 != dir2);
    }
}
