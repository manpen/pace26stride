use crate::instances::instance::Instance;
use crate::job::check_and_extract::SolutionInfos;
use crate::job::job_processor::JobResult;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::warn;

const JSON_KEY_INSTANCE_KEY: &str = "s_key";

const JSON_KEY_SOLVER_PATH: &str = "s_solver_path";
const JSON_KEY_RUN_NAME: &str = "s_run";
const JSON_KEY_INSTANCE_NAME: &str = "s_name";
const JSON_KEY_INSTANCE_PATH: &str = "s_path";
const JSON_KEY_INSTANCE_HASH: &str = "s_idigest";
const JSON_KEY_NUM_TREES: &str = "s_num_trees";
const JSON_KEY_NUM_LEAVES: &str = "s_num_leaves";
const JSON_KEY_JOB_RESULT: &str = "s_result";
const JSON_KEY_SOLUTION_SIZE: &str = "s_score";

const JSON_KEY_PACE_HEURISTIC_SCORE: &str = "s_heuristic_score";

const JSON_KEY_PREV_BEST_KNOWN: &str = "s_prev_best";

/// Maintains a machine-readable log file where each line corresponds to an completed task in JSON format
pub struct SummaryWriter {
    file: Mutex<File>,
    solver_path: Option<PathBuf>,
    run_name: Option<String>,
}

impl SummaryWriter {
    pub async fn new(path: &Path) -> Result<Self, std::io::Error> {
        let file = Mutex::new(File::create_new(path).await?);
        Ok(Self {
            file,
            solver_path: None,
            run_name: None,
        })
    }

    pub fn set_solver_path(&mut self, path: &Path) {
        self.solver_path = Some(path.to_path_buf());
    }

    pub fn set_run_name(&mut self, name: Option<String>) {
        self.run_name = name;
    }

    pub async fn add_entry(
        &self,
        instance: &Instance,
        job_result: JobResult,
        opt_infos: Option<SolutionInfos>,
        prev_best_known: Option<u32>,
        pace_heuristic_score: Option<f64>,
    ) -> Result<(), SummaryWriterError> {
        let mut row = Map::with_capacity(10);

        if let Some(path) = &self.solver_path {
            row.insert(
                JSON_KEY_SOLVER_PATH.into(),
                Value::String(path.to_string_lossy().into()),
            );
        }

        if let Some(run_name) = &self.run_name {
            row.insert(JSON_KEY_RUN_NAME.into(), Value::String(run_name.into()));
        }

        row.insert(
            JSON_KEY_INSTANCE_KEY.into(),
            Value::String(instance.key().into()),
        );
        if let Some(path) = instance.path().as_os_str().to_str() {
            row.insert(JSON_KEY_INSTANCE_PATH.into(), Value::String(path.into()));
        }
        if let Some(idigest) = instance.idigest() {
            row.insert(
                JSON_KEY_INSTANCE_HASH.into(),
                Value::String(idigest.to_string()),
            );
        }

        if let Some(name) = instance.name() {
            row.insert(JSON_KEY_INSTANCE_NAME.into(), Value::String(name.clone()));
        }

        if let Some(trees) = instance.num_trees() {
            row.insert(JSON_KEY_NUM_TREES.into(), trees.into());
        }

        if let Some(leaves) = instance.num_leaves() {
            row.insert(JSON_KEY_NUM_LEAVES.into(), leaves.into());
        }

        if let Some(prev_best) = prev_best_known {
            row.insert(JSON_KEY_PREV_BEST_KNOWN.into(), prev_best.into());
        }

        if let Some(pace_heuristic_score) = pace_heuristic_score {
            row.insert(
                JSON_KEY_PACE_HEURISTIC_SCORE.into(),
                pace_heuristic_score.into(),
            );
        }

        row.insert(
            JSON_KEY_JOB_RESULT.into(),
            Value::String(job_result.to_string()),
        );

        if let JobResult::Valid { size } = job_result {
            row.insert(JSON_KEY_SOLUTION_SIZE.into(), Value::Number(size.into()));
        }

        if let Some((_trees, extra)) = opt_infos {
            for (key, value) in extra {
                let old = row.insert(key.clone(), value);
                if old.is_some() {
                    warn!(
                        "Multiple definitions of key {} in instance {:?}. Use latest",
                        &key,
                        instance.path()
                    );
                }
            }
        }

        let json = serde_json::to_string(&Value::Object(row))?;

        {
            let mut lock = self.file.lock().await;
            lock.write_all(json.as_bytes()).await?;
            lock.write_all("\n".as_bytes()).await?;
            lock.flush().await?;
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum SummaryWriterError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Serde(#[from] serde_json::Error),
}
