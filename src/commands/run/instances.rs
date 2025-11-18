use std::collections::HashSet;
use std::hash::Hash;
use std::path::{Path, PathBuf};

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

#[derive(Default)]
pub struct Instances {
    names: HashSet<String>,
    instances: HashSet<Instance>,
}

impl Instances {
    /// Attempts to insert a new instance fully described by its path;
    /// returns `true` iff the path was not yet in the data set
    pub fn insert_by_path(&mut self, path: PathBuf) -> bool {
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_insert_by_path_new_instance() {
        let mut instances = Instances::default();
        let path = PathBuf::from("/home/user/data/file.txt");

        assert!(instances.insert_by_path(path));
        assert_eq!(instances.len(), 1);
    }

    #[test]
    fn test_insert_by_path_duplicate() {
        let mut instances = Instances::default();
        let path = PathBuf::from("/home/user/data/file.txt");

        assert!(instances.insert_by_path(path.clone()));
        assert!(!instances.insert_by_path(path));
        assert_eq!(instances.len(), 1);
    }

    #[test]
    fn test_insert_by_path_unique_names() {
        let mut instances = Instances::default();
        let path1 = PathBuf::from("/home/user/data/file.txt");
        let path2 = PathBuf::from("/home/user/other/file.txt");

        assert!(instances.insert_by_path(path1));
        assert!(instances.insert_by_path(path2));
        assert_eq!(instances.len(), 2);

        let names: HashSet<String> = instances.into_iter().map(|i| i.name).collect();
        assert!(names.iter().all(|n| !n.is_empty()));
        assert_eq!(names.len(), 2, "{names:?}");
    }
}
