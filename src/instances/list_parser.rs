use pace26checker::digest::digest_output::{DigestError, InstanceDigest};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Eq, PartialEq)]
pub enum Entry {
    StrideInstance(InstanceDigest),
    InstanceFile(PathBuf),
}

pub struct ListEntry {
    pub list_path: Arc<PathBuf>,
    pub list_line: u64,
    pub entry: Entry,
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
pub enum InstanceListParserError {
    #[error("Error when processing file {:?}:{}: {:?}", file, lineno+1, error)]
    IOErrorProcessing {
        error: ProcessingError,
        file: PathBuf,
        lineno: usize,
    },

    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

pub fn parse_instance_list(path: &Path) -> Result<Vec<ListEntry>, InstanceListParserError> {
    let mut entries = Vec::new();

    let list_canon_path = path.canonicalize()?;
    let reader = BufReader::new(File::open(&list_canon_path)?);

    let list_file = Arc::new(list_canon_path.clone());
    let relative_to = list_canon_path
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf();

    for (lineno, line) in reader.lines().enumerate() {
        macro_rules! handle_error {
            ($e:expr) => {
                ($e).map_err(|error| InstanceListParserError::IOErrorProcessing {
                    error: error.into(),
                    file: list_canon_path.clone(),
                    lineno,
                })?
            };
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
        } else if line.starts_with("#s ") {
            // stride digest
            let digest = handle_error!(InstanceDigest::try_from(line.split_at(3).1));

            entries.push(ListEntry {
                list_path: list_file.clone(),
                list_line: lineno as u64,
                entry: Entry::StrideInstance(digest),
            });
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
                entries.push(ListEntry {
                    list_path: list_file.clone(),
                    list_line: lineno as u64,
                    entry: Entry::InstanceFile(path),
                });
            }
        } else {
            // seems to be a file
            let path = PathBuf::from(line);
            entries.push(ListEntry {
                list_path: list_file.clone(),
                list_line: lineno as u64,
                entry: Entry::InstanceFile(path),
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

    fn check_123(list: Vec<ListEntry>) {
        let list_file = path_of_list("123");

        assert_eq!(list.len(), 3);
        assert_eq!(list[0].entry, Entry::InstanceFile("1.nw".into()));
        assert_eq!(list[1].entry, Entry::InstanceFile("2.nw".into()));
        assert_eq!(list[2].entry, Entry::InstanceFile("3.nw".into()));

        for (i, e) in list.iter().enumerate() {
            assert_eq!(e.list_line, 1 + i as u64);
            assert_eq!(e.list_path.as_path(), list_file);
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
            Entry::StrideInstance(
                InstanceDigest::try_from("00089fada00b2f9423de71a49a3675b0").unwrap()
            )
        );
    }

    #[test]
    fn parse_glob() {
        let path = path_of_list("with_glob");
        let list = parse_instance_list(&path).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].entry, Entry::InstanceFile(path));
    }
}
