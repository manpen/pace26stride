use pace26checker::digest::digest_output::InstanceDigest;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InstanceDirectory {
    base: PathBuf,
}

impl InstanceDirectory {
    pub fn new(base: PathBuf) -> Self {
        let base = base.canonicalize().unwrap_or(base);
        InstanceDirectory { base }
    }

    pub fn path(&self) -> &Path {
        &self.base
    }
    pub fn path_of_digest(&self, digest: &InstanceDigest) -> PathBuf {
        let hex = digest.to_string();
        let (shard0, remain) = hex.as_str().split_at(2);
        let (shard1, name) = remain.split_at(2);
        self.base.join(shard0).join(shard1).join(name)
    }
}
