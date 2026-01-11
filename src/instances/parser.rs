use pace26checker::digest::digest_output::{DigestError, InstanceDigest};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum InstanceSource {
    StrideInstance(InstanceDigest),
    InstanceFile(PathBuf),
}

#[derive(Debug, PartialEq, Clone, Error)]
pub enum ReferenceSource {
    File { path: Arc<PathBuf>, lineno: u64 },
    Args { idx: usize },
}

impl Display for ReferenceSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ReferenceSource::File { path, lineno } => {
                write!(f, "{}:{}", path.display(), lineno)
            }
            ReferenceSource::Args { idx } => {
                write!(f, "arg[{}]", idx)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstanceEntry {
    pub entry: InstanceSource,
    pub source: ReferenceSource,
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
        source: ReferenceSource,
    },

    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

pub fn parse_input_arg(inputs: &[PathBuf]) -> Result<Vec<InstanceEntry>, InstanceSourceParser> {
    let mut entries = Vec::new();

    for (idx, input) in inputs.iter().enumerate() {
        if input.starts_with("s:") {
            let digest_string = input.as_os_str().to_string_lossy().to_string();
            let digest =
                InstanceDigest::try_from(digest_string.split_at(2).1).map_err(|error| {
                    InstanceSourceParser::IOErrorProcessing {
                        error: error.into(),
                        source: ReferenceSource::Args { idx },
                    }
                })?;

            entries.push(InstanceEntry {
                source: ReferenceSource::Args { idx },
                entry: InstanceSource::StrideInstance(digest),
            });
        } else if input.extension().is_some_and(|ext| ext == "lst") {
            let mut list = parse_instance_list(input)?;
            entries.append(&mut list);
        } else {
            entries.push(InstanceEntry {
                source: ReferenceSource::Args { idx },
                entry: InstanceSource::InstanceFile(input.clone()),
            });
        }
    }

    Ok(entries)
}

pub fn parse_instance_list(path: &Path) -> Result<Vec<InstanceEntry>, InstanceSourceParser> {
    let mut entries = Vec::new();

    let list_canon_path = path.canonicalize()?;
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
                    source: ReferenceSource::File {
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

            entries.extend(parse_instance_list(&normalized)?);
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
                entries.push(InstanceEntry {
                    source: ReferenceSource::File {
                        path: list_file.clone(),
                        lineno: lineno as u64,
                    },
                    entry: InstanceSource::InstanceFile(path),
                });
            }
        } else if line.starts_with("s:") {
            // stride digest
            let digest = handle_error!(InstanceDigest::try_from(line.split_at(2).1));

            entries.push(InstanceEntry {
                source: ReferenceSource::File {
                    path: list_file.clone(),
                    lineno: lineno as u64,
                },
                entry: InstanceSource::StrideInstance(digest),
            });
        } else {
            // seems to be a file
            let path = PathBuf::from(line);
            entries.push(InstanceEntry {
                source: ReferenceSource::File {
                    path: list_file.clone(),
                    lineno: lineno as u64,
                },
                entry: InstanceSource::InstanceFile(path),
            });
        }
    }

    Ok(entries)
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

    fn check_123(list: Vec<InstanceEntry>) {
        let list_file = Arc::new(path_of_list("123"));

        assert_eq!(list.len(), 3);
        assert_eq!(list[0].entry, InstanceSource::InstanceFile("1.nw".into()));
        assert_eq!(list[1].entry, InstanceSource::InstanceFile("2.nw".into()));
        assert_eq!(list[2].entry, InstanceSource::InstanceFile("3.nw".into()));

        for (i, e) in list.iter().enumerate() {
            assert_eq!(
                e.source,
                ReferenceSource::File {
                    path: list_file.clone(),
                    lineno: 1 + i as u64
                }
            );
        }
    }

    #[test]
    fn parse_123() {
        let list_file = path_of_list("123");
        let list = parse_instance_list(&list_file).unwrap();
        check_123(list);
    }

    #[test]
    fn parse_with_include() {
        let list_file = path_of_list("with_include");
        let list = parse_instance_list(&list_file).unwrap();
        check_123(list);
    }

    #[test]
    fn parse_digest() {
        let list = parse_instance_list(&path_of_list("with_digest")).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].entry,
            InstanceSource::StrideInstance(
                InstanceDigest::try_from("00089fada00b2f9423de71a49a3675b0").unwrap()
            )
        );
    }

    #[test]
    fn parse_glob() {
        let path = path_of_list("with_glob");
        let list = parse_instance_list(&path).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].entry, InstanceSource::InstanceFile(path));
    }
}
