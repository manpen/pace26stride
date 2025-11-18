// TODO: Refactor: JobProcessor should NOT depend on CommandRunArgs !

use thiserror::Error;
use tokio::task::JoinError;
use tracing::error;

use crate::{
    commands::arguments::{self, CommandRunArgs},
    job::{
        check_and_extract::{CheckAndExtract, CheckerError},
        solver_executor::{self, ChildExitStatus, ExecutorError, SolverExecutorBuilder},
    },
    run_directory::{CreateInstanceDirError, RunDirectory},
};
use std::{path::PathBuf, sync::Arc};

#[derive(Error, Debug)]
pub enum JobError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CreateInstanceDir error: {0}")]
    CreateInstanceDirError(#[from] CreateInstanceDirError),

    #[error("Solver execution error: {0}")]
    Executor(#[from] ExecutorError),

    #[error("Checker error: {0}")]
    Checker(#[from] CheckerError),

    #[error("Join error: {0}")]
    JoinError(#[from] JoinError),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobProgress {
    Starting,
    Running,
    Checking,
    Finished,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobResult {
    Valid { size: usize }, // solution size
    Infeasible,
    InvalidInstance,
    SyntaxError,
    SolverError,
    SystemError,
    Timeout,
}

impl JobResult {
    pub fn is_valid(self) -> bool {
        matches!(self, JobResult::Valid { .. })
    }
}

pub struct JobProcessor {
    run_directory: Arc<RunDirectory>,
    arguments: Arc<CommandRunArgs>,
    instance_path: PathBuf,

    progress: JobProgress,
    result: Option<JobResult>,

    solver_exit_status: Option<ChildExitStatus>,
    solution_infos: Vec<(String, serde_json::Value)>,
}

impl JobProcessor {
    pub fn new(
        run_directory: Arc<RunDirectory>,
        arguments: Arc<CommandRunArgs>,
        instance_path: PathBuf,
    ) -> Self {
        Self {
            run_directory,
            arguments,
            instance_path,

            progress: JobProgress::Starting,
            result: None,
            solver_exit_status: None,
            solution_infos: Vec::new(),
        }
    }

    pub fn progress(&self) -> JobProgress {
        self.progress
    }

    pub fn result(&self) -> Option<JobResult> {
        self.result
    }

    pub async fn run(&mut self) -> Result<(), JobError> {
        let result = self.run_internal().await;
        self.progress = JobProgress::Finished;

        if result.is_err() {
            assert!(self.result.is_none());
            self.result = Some(JobResult::SystemError);
        }

        result
    }

    pub async fn run_internal(&mut self) -> Result<(), JobError> {
        let work_dir = self
            .run_directory
            .create_instance_dir_for_path(&self.instance_path)?;
        let solution_path = work_dir.join(solver_executor::PATH_STDOUT);

        let mut executor = SolverExecutorBuilder::default()
            .instance_path(self.instance_path.clone())
            .working_dir(work_dir)
            .solver_path(self.arguments.solver.clone())
            .args(self.arguments.solver_args.clone())
            .env(self.env_vars())
            .timeout(self.arguments.soft_timeout)
            .grace(self.arguments.grace_period)
            .build()
            .expect("Executor Builder failed"); // if this fails it is a programming error and will always fail 

        self.progress = JobProgress::Running;
        let exit_status = executor.run().await?;
        self.solver_exit_status = Some(exit_status);

        if !exit_status.is_success() {
            self.result = Some(match exit_status {
                ChildExitStatus::BeforeTimeout(_) | ChildExitStatus::WithinGrace(_) => {
                    JobResult::SolverError
                }
                ChildExitStatus::Timeout => JobResult::Timeout,
            });

            return Ok(());
        }

        self.check_solution(solution_path).await?;

        Ok(())
    }

    async fn check_solution(&mut self, solution_path: PathBuf) -> Result<(), JobError> {
        self.progress = JobProgress::Checking;
        let instance_path = self.instance_path.clone();

        // pace26checker is implemented in a blocking fashion and may also be CPU-bound; so let's move it into an own thread
        let (solution_infos, result) = tokio::task::spawn_blocking(move || {
            let mut checker = CheckAndExtract::new();
            let result = checker.process(&instance_path, &solution_path);

            let solver_infos = checker.into_solution_infos();

            (solver_infos, result)
        })
        .await?;

        // update solution and map possible error source to job results
        self.solution_infos = solution_infos;
        self.result = Some(match result {
            Ok(size) => JobResult::Valid { size },
            Err(e) => {
                error!("{:?} {:?}", self.instance_path, e);
                map_checker_error_to_job_result(e)
            }
        });
        Ok(())
    }

    fn env_vars(&self) -> Vec<(String, String)> {
        vec![
            (
                "STRIDE_INSTANCE_PATH".to_string(),
                self.instance_path.to_string_lossy().to_string(),
            ),
            (
                arguments::ENV_SOFT_TIMEOUT.to_string(),
                format!("{}", self.arguments.soft_timeout.as_secs_f64()),
            ),
            (
                arguments::ENV_GRACE_PERIOD.to_string(),
                format!("{}", self.arguments.grace_period.as_secs_f64()),
            ),
        ]
    }
}

fn map_checker_error_to_job_result(e: CheckerError) -> JobResult {
    match e {
        CheckerError::Io(..) => JobResult::SystemError,
        CheckerError::CreateInstanceDirError(..) => JobResult::SystemError,
        CheckerError::InstanceInputError(..) => JobResult::InvalidInstance,
        CheckerError::SolutionInputError(..) => JobResult::SyntaxError,
        CheckerError::ForestConstructionError(..) => JobResult::InvalidInstance,
        CheckerError::SolutionTreeMatchingError { .. } => JobResult::Infeasible,
    }
}
