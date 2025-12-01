use std::{fs::File, io::Write, path::Path};

use serde_json::{Map, Value};
use thiserror::Error;
use tracing::warn;

use crate::{
    commands::run::instances::Instance,
    job::job_processor::{JobResult, SolutionInfos},
};

const JSON_KEY_INSTANCE_NAME: &str = "name";
const JSON_KEY_INSTANCE_PATH: &str = "path";
const JSON_KEY_INSTANCE_HASH: &str = "stride_hash";
const JSON_KEY_JOB_RESULT: &str = "result";
const JSON_KEY_SOLUTION_SIZE: &str = "score";

/// Maintains a machine-readable log file where each line corresponds to an completed task in JSON format
pub struct SummaryWriter {
    file: File,
}

impl SummaryWriter {
    pub fn new(path: &Path) -> Result<Self, std::io::Error> {
        let file = File::create_new(path)?;
        Ok(Self { file })
    }

    pub fn add_entry(
        &mut self,
        instance: &Instance,
        job_result: JobResult,
        opt_infos: Option<SolutionInfos>,
    ) -> Result<(), SummaryWriterError> {
        let mut row = Map::with_capacity(10);

        row.insert(
            JSON_KEY_INSTANCE_NAME.into(),
            Value::String(instance.name().into()),
        );
        if let Some(path) = instance.path().as_os_str().to_str() {
            row.insert(JSON_KEY_INSTANCE_PATH.into(), Value::String(path.into()));
        }
        if let Some(hash) = instance.stride_hash() {
            row.insert(JSON_KEY_INSTANCE_HASH.into(), Value::String(hash.into()));
        }

        row.insert(
            JSON_KEY_JOB_RESULT.into(),
            Value::String(job_result.to_string()),
        );

        if let JobResult::Valid { size } = job_result {
            row.insert(JSON_KEY_SOLUTION_SIZE.into(), Value::Number(size.into()));
        }

        if let Some(extra) = opt_infos {
            for (key, value) in extra {
                let old = row.insert(format!("s_{key}"), value);
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
        writeln!(&mut self.file, "{json}")?;
        let _ = self.file.flush(); // ignore errors here, they might be recoverable

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
