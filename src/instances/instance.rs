use crate::commands::arguments::InstanceOrder;
use crate::instances::directory::InstanceDirectory;
use crate::instances::parser::{InstanceSource, InstanceSourceDescriptor};
use console::Style;
use pace26checker::digest::digest_output::InstanceDigest;
use pace26io::pace::reader::{Action, InstanceReader, InstanceVisitor, ReaderError};
use rand::prelude::SliceRandom;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::io::{BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::info;

#[derive(Debug, Clone)]
pub struct Instance {
    key: String,
    path: PathBuf,
    idigest: Option<InstanceDigest>,
    name: Option<String>,
    num_trees: Option<usize>,
    num_leaves: Option<usize>,
}

impl Instance {
    pub fn key(&self) -> &str {
        self.key.as_str()
    }

    pub fn idigest(&self) -> Option<&InstanceDigest> {
        self.idigest.as_ref()
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
    pub fn name(&self) -> Option<&String> {
        self.name.as_ref()
    }

    pub fn num_trees(&self) -> Option<usize> {
        self.num_trees
    }
    pub fn num_leaves(&self) -> Option<usize> {
        self.num_leaves
    }

    pub fn display_name(&self, max_len: usize) -> String {
        let mut result = String::with_capacity(max_len);

        if let Some(name) = &self.name {
            result.extend(name.chars().take(max_len));
        }

        if let Some(digest) = self.idigest {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(&digest.to_string());
        }

        if let Some(stem) = self.path.file_stem() {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(&stem.to_string_lossy());
        }

        if result.len() > max_len {
            result = result.chars().take(max_len).collect();
        }

        result
    }
}

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    PaceReader(#[from] ReaderError),

    #[error("File {} has idigest {} instead of expect {}", .path.display(), actual, expected)]
    DigestMismatch {
        path: PathBuf,
        actual: InstanceDigest,
        expected: InstanceDigest,
    },

    #[error(
        "Could not find STRIDE instance {}. Check argument {} argument or use subcommand {} to fetch missing files",
        Style::new().red().apply_to(.0.to_string()),
        Style::new().yellow().apply_to("--downloads_path"),
        Style::new().yellow().apply_to("download"),
    )]
    StrideInstanceNotFound(InstanceDigest),

    #[error("Cannot find instance at path {}", Style::new().red().apply_to(.0.to_string_lossy().to_string()))]
    InstanceNotFound(PathBuf),
}

impl Instance {
    pub fn try_new_from_path(path: &Path) -> Result<Self, InstanceError> {
        let path = match path.canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return Err(InstanceError::InstanceNotFound(path.to_path_buf()));
            }
            Err(e) => return Err(InstanceError::IO(e)),
        };

        Self::construct_from_path(path)
    }

    pub fn try_new_from_idigest(
        instance_dir: &InstanceDirectory,
        idigest: InstanceDigest,
    ) -> Result<Self, InstanceError> {
        let path = match instance_dir.path_of_digest(&idigest).canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return Err(InstanceError::StrideInstanceNotFound(idigest));
            }
            Err(e) => return Err(InstanceError::IO(e)),
        };

        let mut instance = Self::construct_from_path(path.clone())?;

        if let Some(inst_idigest) = &instance.idigest {
            if inst_idigest != &idigest {
                return Err(InstanceError::DigestMismatch {
                    path,
                    actual: *inst_idigest,
                    expected: idigest,
                });
            }
        } else {
            instance.idigest = Some(idigest);
        }

        Ok(instance)
    }

    fn construct_from_path(path: PathBuf) -> Result<Self, InstanceError> {
        let file = BufReader::new(std::fs::File::open(&path)?);
        let mut key = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string());

        let mut visitor = InstanceHeaderVisitor::default();
        InstanceReader::new(&mut visitor).read(file)?;

        if let Some(digest) = &visitor.idigest
            && key.len() > 4
        {
            // due to sharding the filestem only accounts for parts of the idigest.
            // in such a situation, we replace the key with the idgest
            let idigest = digest.to_string();
            if idigest.ends_with(&key) {
                key = idigest;
            }
        }

        Ok(Self {
            path,
            key,
            idigest: visitor.idigest,
            name: visitor.name,
            num_leaves: visitor.num_leaves,
            num_trees: visitor.num_trees,
        })
    }
}

pub fn collect_instances(
    instance_dir: &InstanceDirectory,
    entries: Vec<InstanceSourceDescriptor>,
) -> Result<Vec<Instance>, InstanceError> {
    let mut instances = convert_instance_entries_into_instances(instance_dir, entries)?;
    dedup_instances(&mut instances);
    make_instances_keys_unique(&mut instances);

    Ok(instances)
}

fn convert_instance_entries_into_instances(
    instance_dir: &InstanceDirectory,
    entries: Vec<InstanceSourceDescriptor>,
) -> Result<Vec<Instance>, InstanceError> {
    let mut instances: Vec<Instance> = Vec::with_capacity(entries.len());

    for inst in entries {
        let res = match inst.instance_source {
            InstanceSource::StrideInstance(i) => Instance::try_new_from_idigest(instance_dir, i),
            InstanceSource::InstanceFile(p) => Instance::try_new_from_path(&p),
        };

        instances.push(res?);
    }
    Ok(instances)
}

fn make_instances_keys_unique(instances: &mut Vec<Instance>) {
    let mut keys: HashMap<String, usize> = HashMap::with_capacity(instances.len());
    for inst in instances {
        loop {
            let count = *keys
                .entry(inst.key.clone())
                .and_modify(|c| *c += 1)
                .or_insert(1);
            if count > 1 {
                inst.key = format!("{}_{count}", inst.key());
            } else {
                break;
            }
        }
    }
}

fn dedup_instances(instances: &mut Vec<Instance>) {
    let len_before = instances.len();
    instances.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    instances.dedup_by(|a, b| a.path == b.path);

    if len_before != instances.len() {
        info!(
            "Removed {} duplicates from instance list",
            len_before - instances.len()
        );
    }
}

pub fn sort_instances(instances: &mut [Instance], order: InstanceOrder) {
    // we later pop the instances from the back; hence the order here is flipped!
    match order {
        InstanceOrder::Random => {
            instances.shuffle(&mut rand::rng());
        }
        InstanceOrder::NodesTreesAsc => {
            instances.sort_unstable_by_key(|i| {
                Reverse((
                    i.num_leaves.unwrap_or(usize::MAX),
                    i.num_trees.unwrap_or(usize::MAX),
                ))
            });
        }
        InstanceOrder::NodesTreesDesc => {
            instances.sort_unstable_by_key(|i| {
                (
                    i.num_leaves.unwrap_or(usize::MAX),
                    i.num_trees.unwrap_or(usize::MAX),
                )
            });
        }
        InstanceOrder::TreesNodesAsc => {
            instances.sort_unstable_by_key(|i| {
                Reverse((
                    i.num_trees.unwrap_or(usize::MAX),
                    i.num_leaves.unwrap_or(usize::MAX),
                ))
            });
        }
        InstanceOrder::TreesNodesDesc => {
            instances.sort_unstable_by_key(|i| {
                (
                    i.num_trees.unwrap_or(usize::MAX),
                    i.num_leaves.unwrap_or(usize::MAX),
                )
            });
        }
    }
}

#[derive(Default)]
struct InstanceHeaderVisitor {
    idigest: Option<InstanceDigest>,
    name: Option<String>,
    num_trees: Option<usize>,
    num_leaves: Option<usize>,
}

impl InstanceVisitor for InstanceHeaderVisitor {
    fn visit_header(&mut self, _lineno: usize, num_trees: usize, num_leaves: usize) -> Action {
        self.num_trees = Some(num_trees);
        self.num_leaves = Some(num_leaves);
        Action::Terminate
    }
    fn visit_stride_line(&mut self, _lineno: usize, _line: &str, key: &str, value: &str) -> Action {
        match key {
            "name" => {
                self.name = serde_json::from_str(value).ok();
            }
            "idigest" => {
                self.idigest = serde_json::from_str(value).ok();
            }
            _ => {}
        }

        Action::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::parser::DescriptorSource;
    use crate::test_helpers::test_testcases_dir;

    #[test]
    fn instance_not_found() {
        let idigest = InstanceDigest::try_from("0000926dfbc847ee6009bf51ada7dead").unwrap();
        let instance_dir = InstanceDirectory::new(test_testcases_dir().join("downloads"));

        let res = collect_instances(
            &instance_dir,
            vec![InstanceSourceDescriptor {
                instance_source: InstanceSource::StrideInstance(idigest),
                entry_source: DescriptorSource::Args { idx: 0 },
            }],
        );

        assert!(
            matches!(
                res.as_ref().err().unwrap(),
                InstanceError::StrideInstanceNotFound(..)
            ),
            "{res:?}"
        );
    }

    #[test]
    fn read_with_name() {
        let idigest = InstanceDigest::try_from("0000926dfbc847ee6009bf51ada7bc2b").unwrap();
        let instance_dir = InstanceDirectory::new(test_testcases_dir().join("downloads"));

        let instances = collect_instances(
            &instance_dir,
            vec![InstanceSourceDescriptor {
                instance_source: InstanceSource::StrideInstance(idigest),
                entry_source: DescriptorSource::Args { idx: 0 },
            }],
        )
        .unwrap();

        assert_eq!(instances.len(), 1);

        let instance = &instances[0];
        assert_eq!(instance.idigest(), Some(&idigest));
        assert_eq!(instance.name(), Some(&"NAMEtestNAME".into()));
        assert_eq!(instance.display_name(12), String::from("NAMEtestNAME"));
    }

    #[test]
    fn read_without_name() {
        const IDIGEST: &str = "000a71f690826e1f87cbb87fc94e4613";
        let idigest = InstanceDigest::try_from(IDIGEST).unwrap();
        let instance_dir = InstanceDirectory::new(test_testcases_dir().join("downloads"));

        let instances = collect_instances(
            &instance_dir,
            vec![InstanceSourceDescriptor {
                instance_source: InstanceSource::StrideInstance(idigest),
                entry_source: DescriptorSource::Args { idx: 0 },
            }],
        )
        .unwrap();

        assert_eq!(instances.len(), 1);

        let instance = &instances[0];
        assert_eq!(instance.idigest(), Some(&idigest));
        assert!(instance.name().is_none());
        assert_eq!(instance.display_name(IDIGEST.len()), idigest.to_string());
    }
}
