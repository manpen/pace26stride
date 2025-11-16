use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};
use thiserror::Error;

use pace26checker::{
    checks::bin_forest::{BinForest, TreeInsertionError},
    io::{
        instance_reader::{self, *},
        solution_reader::*,
    },
};
use tracing::{error, warn};

#[derive(Default)]
pub struct CheckAndExtract {
    instance_path: PathBuf,

    instance_trees: Vec<(usize, instance_reader::Tree)>,
    instance_num_leaves: u32,
    instance_infos: HashMap<String, serde_json::Value>,

    solution_infos: Vec<(String, serde_json::Value)>,
    solution_forest: Vec<(usize, instance_reader::Tree)>,
}

#[derive(Error, Debug)]
pub enum CheckerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Instance input error: {0}")]
    InstanceInputError(#[from] InstanceVisitorError),

    #[error("Solution input error: {0}")]
    SolutionInputError(#[from] SolutionVisitorError),

    #[error("Solution input error: {0}")]
    ForestConstructionError(#[from] TreeInsertionError),

    #[error("Failed to match solution tree in line {} against instance tree in line {}", instance_line + 1, solution_lineno + 1)]
    SolutionTreeMatchingError {
        instance_line: usize,
        solution_lineno: usize,
    },
}

impl CheckAndExtract {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(
        &mut self,
        instance_path: &Path,
        solution_path: &Path,
    ) -> Result<usize, CheckerError> {
        self.read_instance(instance_path)?;
        self.read_solution(solution_path)?;

        self.check_solution()
    }

    pub fn into_solution_infos(self) -> Vec<(String, serde_json::Value)> {
        self.solution_infos
    }

    fn read_instance(&mut self, path: &Path) -> Result<(), CheckerError> {
        self.instance_path = path.to_path_buf();

        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut visitor = InstanceInputVisitor::process(&mut reader);

        for e in &visitor.errors {
            error!("[{:?}] {e:?}", self.instance_path);
        }
        for w in &visitor.warnings {
            warn!("[{:?}] {w:?}", self.instance_path);
        }

        if !visitor.errors.is_empty() {
            return Err(CheckerError::InstanceInputError(visitor.errors.remove(0)));
        }

        self.instance_num_leaves = visitor.header.unwrap().1; // safe since the reader would raise an InstanceInputError::NoHeader error if there is no header

        self.instance_trees = std::mem::take(&mut visitor.trees);
        for (key, value) in visitor.stride_lines {
            self.instance_infos.insert(key, value);
        }

        Ok(())
    }

    fn read_solution(&mut self, path: &Path) -> Result<(), CheckerError> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut visitor = SolutionInputVisitor::process(&mut reader, self.instance_num_leaves);

        for e in &visitor.errors {
            error!("[{:?}] {e:?}", self.instance_path);
        }
        for w in &visitor.warnings {
            warn!("[{:?}] {w:?}", self.instance_path);
        }

        self.solution_infos = std::mem::take(&mut visitor.stride_lines);
        self.solution_forest = std::mem::take(&mut visitor.trees);

        Ok(())
    }

    fn check_solution(&mut self) -> Result<usize, CheckerError> {
        let solution_size = self.solution_forest.len();
        for (instance_lineno, instance_tree) in std::mem::take(&mut self.solution_forest) {
            let mut forest = BinForest::new(self.instance_num_leaves);
            forest = forest.add_tree(instance_tree.clone())?;

            for (sol_line, subtree) in &self.solution_forest {
                if let Some(f) = forest.isolate_tree(subtree) {
                    forest = f;
                } else {
                    return Err(CheckerError::SolutionTreeMatchingError {
                        instance_line: instance_lineno,
                        solution_lineno: *sol_line,
                    });
                }
            }
        }
        Ok(solution_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn glob_pattern(key: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testcases")
            .join(key)
            .join("*.in")
    }

    #[test]
    fn test_valid_solutions() {
        let pattern = glob_pattern("valid_instance_solution_pairs");
        let instances = glob::glob(pattern.to_str().unwrap()).unwrap();

        let mut num_tests = 0;
        for instance_path in instances {
            let instance_path = instance_path.unwrap();
            let solution_path = instance_path.with_extension("out");

            let mut checker = CheckAndExtract::new();
            let result = checker.process(&instance_path, &solution_path);

            assert!(
                result.is_ok(),
                "Expected valid solution for instance {:?}, but got error: {:?}",
                instance_path,
                result.err()
            );

            num_tests += 1;
        }

        assert!(
            num_tests > 0,
            "No valid instance-solution pairs found for testing"
        );
    }
}
