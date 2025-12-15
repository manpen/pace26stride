use serde_json::{Map, Value};
use std::path::Path;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::warn;

use crate::job::check_and_extract::SolutionInfos;
use crate::{commands::run::instances::Instance, job::job_processor::JobResult};

const JSON_KEY_INSTANCE_NAME: &str = "s_name";
const JSON_KEY_INSTANCE_PATH: &str = "s_path";
const JSON_KEY_INSTANCE_HASH: &str = "s_idigest";
const JSON_KEY_JOB_RESULT: &str = "s_result";
const JSON_KEY_SOLUTION_SIZE: &str = "s_score";

const JSON_KEY_PREV_BEST_KNOWN: &str = "s_prev_best";

/// Maintains a machine-readable log file where each line corresponds to an completed task in JSON format
pub struct SummaryWriter {
    file: Mutex<File>,
}

impl SummaryWriter {
    pub async fn new(path: &Path) -> Result<Self, std::io::Error> {
        let file = Mutex::new(File::create_new(path).await?);
        Ok(Self { file })
    }

    pub async fn add_entry(
        &self,
        instance: &Instance,
        job_result: JobResult,
        opt_infos: Option<SolutionInfos>,
        prev_best_known: Option<u32>,
    ) -> Result<(), SummaryWriterError> {
        let mut row = Map::with_capacity(10);

        row.insert(
            JSON_KEY_INSTANCE_NAME.into(),
            Value::String(instance.name().into()),
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
        if let Some(prev_best) = prev_best_known {
            row.insert(
                JSON_KEY_PREV_BEST_KNOWN.into(),
                Value::String(prev_best.to_string()),
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
            lock.write("\n".as_bytes()).await?;
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
