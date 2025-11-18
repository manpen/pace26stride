use std::collections::HashSet;
use std::fs::File;
use std::hash::Hash;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Clone, Debug, Eq)]
pub struct Instance {
    name: String,
    path: PathBuf,
}

impl Hash for Instance {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

impl PartialEq for Instance {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Instance {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Error, Debug)]
pub enum InstancesError {
    #[error("Path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("Path points to directory: {0}")]
    DirectoryFound(PathBuf),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Default, Debug, Clone)]
pub struct Instances {
    names: HashSet<String>,
    instances: HashSet<Instance>,
}

impl Instances {
    pub fn parse_and_insert_path(&mut self, path: &Path) -> Result<(), InstancesError> {
        if path.is_dir() {
            return Err(InstancesError::DirectoryFound(path.to_path_buf()));
        }

        if path.extension().and_then(|e| e.to_str()) == Some("lst") {
            debug!("Interpret path {path:?} as list");
            self.insert_from_list_file(path)
        } else {
            debug!("Interpret path {path:?} as instance");
            self.insert_instace_by_path(path.to_owned());
            Ok(())
        }
    }

    pub fn insert_from_list_file(&mut self, path: &Path) -> Result<(), InstancesError> {
        let file = File::open(path)?;
        let canon_path = path.canonicalize()?;
        let relative_to = canon_path
            .parent()
            .expect("Parent needs to exists, since path is canonical");
        self.insert_from_list(BufReader::new(file), relative_to)
    }

    pub fn insert_from_list(
        &mut self,
        reader: impl BufRead,
        relative_to: &Path,
    ) -> Result<(), InstancesError> {
        for line in reader.lines() {
            let line = if let Ok(line) = line {
                line
            } else {
                continue;
            };

            let line = line.trim();

            if line.is_empty() || line.starts_with("#") {
                continue;
            }

            let canonical = if line.starts_with('/') {
                PathBuf::from(line)
            } else if let Some(c) = normalize_path(&relative_to.join(line)) {
                c
            } else {
                warn!("Failed to canonicalize line `{line}`");
                continue;
            };

            if let Some(pattern) = canonical.to_str()
                && (pattern.contains('*') || pattern.contains('?'))
            {
                // treat as glob string
                debug!("glob {pattern}");
                match glob::glob(pattern) {
                    Ok(paths) => {
                        for p in paths.filter_map(|p| p.ok()) {
                            self.parse_and_insert_path(&p)?;
                        }
                    }
                    Err(e) => {
                        warn!("Pattern error: {e}");
                        continue;
                    }
                }
            } else {
                self.parse_and_insert_path(&canonical)?;
            }
        }

        Ok(())
    }

    /// Attempts to insert a new instance fully described by its path;
    /// returns `true` iff the path was not yet in the data set
    pub fn insert_instace_by_path(&mut self, path: PathBuf) -> bool {
        // we optimize for the good case, where the path is new
        let name = self.unique_name_from_path(&path);
        let newly_inserted = self.instances.insert(Instance {
            path,
            name: name.clone(),
        });

        if !newly_inserted {
            self.names.remove(&name);
        }

        newly_inserted
    }

    /// Returns number of elements in collection
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Returns `true` iff there are no elements in the collection
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Instance> {
        self.instances.iter()
    }

    /// Constructs a unique name `filestem_parent_parent_parent...` where a minimal
    /// number of parents is select; if a complete traversal of parents does not yet
    /// yield a unique name, a number suffix is added using [`Instances::unique_by_counter`]
    fn unique_name_from_path(&mut self, mut path: &Path) -> String {
        let mut name = if let Some(stem) = path.file_stem() {
            String::from(stem.to_string_lossy())
        } else {
            return self.unique_by_counter("unnamed");
        };

        loop {
            if self.names.insert(name.clone()) {
                return name;
            }

            if let Some(parent) = path.parent()
                && let Some(parent_name) = parent.file_name()
            {
                path = parent;
                name.push('_');
                name.push_str(&parent_name.to_string_lossy());
            } else {
                // this case exist as a fallback if for some future case, we do not want to deal with absolute paths
                return self.unique_by_counter(&name);
            }
        }
    }

    /// Constructs a unique name by adding a numeric suffix `-i` where i>2 is the
    /// smallest possible choice
    fn unique_by_counter(&mut self, name_prefix: &str) -> String {
        for i in 2usize.. {
            let cand = format!("{name_prefix}-{i}");
            if self.names.insert(cand.clone()) {
                return cand;
            }
        }
        unreachable!()
    }
}

impl IntoIterator for Instances {
    type Item = Instance;

    type IntoIter = <HashSet<Instance> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.instances.into_iter()
    }
}

fn normalize_path(path: &Path) -> Option<PathBuf> {
    let mut stack: Vec<_> = Vec::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                stack.pop()?;
            }
            Component::CurDir => {}
            Component::Normal(s) => stack.push(s),
            Component::RootDir => stack.clear(),
            _ => {}
        }
    }

    // TODO: we could count / reserve the storage required before
    let mut out = if path.is_absolute() {
        PathBuf::from("/")
    } else {
        PathBuf::new()
    };

    for p in stack {
        out = out.join(p)
    }

    Some(out)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_helpers::*;
    use tracing_test::traced_test;

    #[test]
    fn test_insert_by_path_new_instance() {
        let mut instances = Instances::default();
        let path = PathBuf::from("/home/user/data/file.txt");

        assert!(instances.insert_instace_by_path(path));
        assert_eq!(instances.len(), 1);
    }

    #[test]
    fn test_insert_by_path_duplicate() {
        let mut instances = Instances::default();
        let path = PathBuf::from("/home/user/data/file.txt");

        assert!(instances.insert_instace_by_path(path.clone()));
        assert!(!instances.insert_instace_by_path(path));
        assert_eq!(instances.len(), 1);
    }

    #[test]
    fn test_insert_by_path_unique_names() {
        let mut instances = Instances::default();
        let path1 = PathBuf::from("/home/user/data/file.txt");
        let path2 = PathBuf::from("/home/user/other/file.txt");

        assert!(instances.insert_instace_by_path(path1));
        assert!(instances.insert_instace_by_path(path2));
        assert_eq!(instances.len(), 2);

        let names: HashSet<String> = instances.into_iter().map(|i| i.name).collect();
        assert!(names.iter().all(|n| !n.is_empty()));
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    #[traced_test]
    fn test_insert_from_list_files_only() {
        let list = String::from("#comment\nfoo/bar.nw\nbar/bar.nw\n/fizz/buzz.nw\n#comment\n");
        let mut instances = Instances::default();
        instances
            .insert_from_list(list.as_bytes(), &PathBuf::from("/tmp/"))
            .unwrap();

        assert_eq!(instances.len(), 3);
    }

    #[test]
    #[traced_test]
    fn test_insert_from_list_pattern() {
        let list = "**/*.in\n*/*.out";
        let relative_to = test_manifest_dir().join("testcases");

        let mut instances = Instances::default();
        instances
            .insert_from_list(list.as_bytes(), &relative_to)
            .unwrap();

        assert!(
            instances.len() > 80,
            "{instances:?} -- len: {}",
            instances.len()
        );

        assert!(
            instances
                .iter()
                .any(|i| i.path().extension().and_then(|x| x.to_str()) == Some("in"))
        );
        assert!(
            instances
                .iter()
                .any(|i| i.path().extension().and_then(|x| x.to_str()) == Some("out"))
        );
    }

    #[test]
    #[traced_test]

    fn test_insert_from_list_file() {
        let mut instances = Instances::default();
        instances
            .insert_from_list_file(&test_testcases_dir().join("test.lst"))
            .unwrap();
        assert!(instances.len() > 3);
    }

    #[test]
    #[traced_test]

    fn test_insert_from_list_file_recursive() {
        let mut instances = Instances::default();
        instances
            .insert_from_list_file(&test_testcases_dir().join("test_recursive_list.lst"))
            .unwrap();
        assert!(instances.len() > 3, "{instances:?}");
    }
}
