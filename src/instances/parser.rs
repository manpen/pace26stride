use pace26checker::digest::digest_output::{DigestError, InstanceDigest};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct InstanceSourceDescriptor {
    pub instance_source: InstanceSource,
    pub entry_source: DescriptorSource,
}

impl InstanceSourceDescriptor {
    pub fn path(&self) -> Option<&Path> {
        match &self.instance_source {
            InstanceSource::InstanceFile(p) => Some(p.as_path()),
            InstanceSource::StrideInstance(..) => None,
        }
    }

    pub fn digest(&self) -> Option<&InstanceDigest> {
        match &self.instance_source {
            InstanceSource::InstanceFile(..) => None,
            InstanceSource::StrideInstance(d) => Some(d),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProcessingError {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Digest(#[from] DigestError),
    #[error(transparent)]
    Glob(#[from] glob::GlobError),
    #[error(transparent)]
    Pattern(#[from] glob::PatternError),
}

#[derive(Debug, Error)]
pub enum InstanceSourceParser {
    #[error("Error when processing. Got {:?}", error)]
    IOErrorProcessing {
        error: ProcessingError,
        source: DescriptorSource,
    },

    #[error(transparent)]
    IOError(#[from] std::io::Error),

    #[error("Cannot find list at path {}", console::Style::new().red().apply_to(.0.to_string_lossy().to_string()))]
    ListFileNotFound(PathBuf),
}

pub fn collect_instances_from_args(
    inputs: &[PathBuf],
) -> Result<Vec<InstanceSourceDescriptor>, InstanceSourceParser> {
    let mut entries = Vec::new();

    for (idx, input) in inputs.iter().enumerate() {
        if input
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("s:"))
        {
            let digest_string = input.as_os_str().to_string_lossy().to_string();
            let digest =
                InstanceDigest::try_from(digest_string.split_at(2).1).map_err(|error| {
                    InstanceSourceParser::IOErrorProcessing {
                        error: error.into(),
                        source: DescriptorSource::Args { idx },
                    }
                })?;

            entries.push(InstanceSourceDescriptor {
                entry_source: DescriptorSource::Args { idx },
                instance_source: InstanceSource::StrideInstance(digest),
            });
        } else if input.extension().is_some_and(|ext| ext == "lst") {
            let mut list = parse_instance_list_file(input)?;
            entries.append(&mut list);
        } else {
            entries.push(InstanceSourceDescriptor {
                entry_source: DescriptorSource::Args { idx },
                instance_source: InstanceSource::InstanceFile(input.clone()),
            });
        }
    }

    Ok(entries)
}

pub fn parse_instance_list_file(
    path: &Path,
) -> Result<Vec<InstanceSourceDescriptor>, InstanceSourceParser> {
    let mut entries = Vec::new();

    let list_canon_path = match path.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(InstanceSourceParser::ListFileNotFound(path.into()));
        }
        Err(e) => return Err(InstanceSourceParser::IOError(e)),
    };

    let reader = BufReader::new(File::open(&list_canon_path)?);

    let relative_to = list_canon_path
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf();
    let list_file = Arc::new(list_canon_path);

    for (lineno, line) in reader.lines().enumerate() {
        macro_rules! handle_error {
            ($e:expr) => {{
                let list_file_error_clone = list_file.clone();
                ($e).map_err(move |error| InstanceSourceParser::IOErrorProcessing {
                    error: error.into(),
                    source: DescriptorSource::File {
                        path: list_file_error_clone.clone(),
                        lineno: lineno as u64,
                    },
                })?
            }};
        }

        let Ok(line) = line else { continue };
        let line = line.trim();

        if line.is_empty() || line.starts_with("# ") {
            // this is a comment, nothing else to do
        } else if line.starts_with("#i ") {
            // include list
            let path = PathBuf::from(line.split_at(3).1);
            let normalized = if path.is_absolute() {
                path
            } else {
                relative_to.join(path)
            };

            entries.extend(parse_instance_list_file(&normalized)?);
        } else if line.starts_with("#g ") {
            // glob pattern
            let pattern = PathBuf::from(line.split_at(3).1);

            let norm_pattern = if pattern.is_absolute() {
                pattern
            } else {
                relative_to.join(pattern)
            }
            .as_os_str()
            .to_string_lossy()
            .to_string();

            let paths = handle_error!(glob::glob(&norm_pattern));
            for path in paths {
                let path = handle_error!(path);
                entries.push(InstanceSourceDescriptor {
                    entry_source: DescriptorSource::File {
                        path: list_file.clone(),
                        lineno: lineno as u64,
                    },
                    instance_source: InstanceSource::InstanceFile(path),
                });
            }
        } else if line.starts_with("s:") {
            // stride digest
            let digest = handle_error!(InstanceDigest::try_from(line.split_at(2).1));

            entries.push(InstanceSourceDescriptor {
                entry_source: DescriptorSource::File {
                    path: list_file.clone(),
                    lineno: lineno as u64,
                },
                instance_source: InstanceSource::StrideInstance(digest),
            });
        } else {
            // seems to be a file
            let mut path = PathBuf::from(line);

            if !path.is_absolute() {
                path = relative_to.join(path);
            }

            entries.push(InstanceSourceDescriptor {
                entry_source: DescriptorSource::File {
                    path: list_file.clone(),
                    lineno: lineno as u64,
                },
                instance_source: InstanceSource::InstanceFile(path),
            });
        }
    }

    Ok(entries)
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum InstanceSource {
    StrideInstance(InstanceDigest),
    InstanceFile(PathBuf),
}

#[derive(Debug, PartialEq, Clone, Error)]
pub enum DescriptorSource {
    File { path: Arc<PathBuf>, lineno: u64 },
    Args { idx: usize },
}

impl Display for DescriptorSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DescriptorSource::File { path, lineno } => {
                write!(f, "{}:{}", path.display(), lineno)
            }
            DescriptorSource::Args { idx } => {
                write!(f, "arg[{}]", idx)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_testcases_dir;

    fn path_of_list(key: &str) -> PathBuf {
        let mut path = test_testcases_dir().join("lists").join(key);
        path.set_extension("lst");
        path
    }

    fn check_123(list: Vec<InstanceSourceDescriptor>) {
        let list_file = Arc::new(path_of_list("123"));

        assert_eq!(list.len(), 3);
        assert_eq!(
            list[0].path().unwrap().file_name().unwrap(),
            &PathBuf::from("1.nw")
        );
        assert_eq!(
            list[1].path().unwrap().file_name().unwrap(),
            &PathBuf::from("2.nw")
        );
        assert_eq!(
            list[2].path().unwrap().file_name().unwrap(),
            &PathBuf::from("3.nw")
        );

        for (i, e) in list.iter().enumerate() {
            assert_eq!(
                e.entry_source,
                DescriptorSource::File {
                    path: list_file.clone(),
                    lineno: 1 + i as u64
                }
            );
        }
    }

    #[test]
    fn parse_123() {
        let list_file = path_of_list("123");
        let list = parse_instance_list_file(&list_file).unwrap();
        check_123(list);
    }

    #[test]
    fn parse_with_include() {
        let list_file = path_of_list("with_include");
        let list = parse_instance_list_file(&list_file).unwrap();
        check_123(list);
    }

    #[test]
    fn parse_digest() {
        let list = parse_instance_list_file(&path_of_list("with_digest")).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].instance_source,
            InstanceSource::StrideInstance(
                InstanceDigest::try_from("00089fada00b2f9423de71a49a3675b0").unwrap()
            )
        );
    }

    #[test]
    fn parse_glob() {
        let path = path_of_list("with_glob");
        let list = parse_instance_list_file(&path).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].instance_source, InstanceSource::InstanceFile(path));
    }

    #[test]
    fn parse_summary_list() {
        let path = test_testcases_dir().join("test_summary.lst");
        let list = parse_instance_list_file(&path).unwrap();
        assert_eq!(list.len(), 14);

        for inst in list {
            let InstanceSource::InstanceFile(path) = inst.instance_source else {
                panic!()
            };
            assert!(path.exists(), "File does not exist: {}", path.display());
        }
    }
}
