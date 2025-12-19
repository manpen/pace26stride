use derive_builder::Builder;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::task::JoinError;
use tracing::{debug, error, trace};

use crate::job::check_and_extract::SolutionInfos;
use crate::{
    commands::arguments,
    job::{
        check_and_extract::{CheckAndExtract, CheckerError},
        solver_executor::{self, ChildExitStatus, ExecutorError, SolverExecutorBuilder},
    },
    run_directory::{CreateInstanceDirError, RunDirectory},
};
use std::fmt::Display;
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum JobProgress {
    #[default]
    Starting = 0,
    Running = 1,
    Checking = 2,
    Finished = 3,
}

struct AtomicJobProgress {
    value: AtomicUsize,
}

impl Default for AtomicJobProgress {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl Clone for AtomicJobProgress {
    fn clone(&self) -> Self {
        Self::new(self.load())
    }
}

impl AtomicJobProgress {
    fn new(state: JobProgress) -> Self {
        Self {
            value: AtomicUsize::new(state as usize),
        }
    }

    fn load(&self) -> JobProgress {
        match self.value.load(Ordering::Acquire) {
            x if x == JobProgress::Starting as usize => JobProgress::Starting,
            x if x == JobProgress::Running as usize => JobProgress::Running,
            x if x == JobProgress::Checking as usize => JobProgress::Checking,
            x if x == JobProgress::Finished as usize => JobProgress::Finished,
            _ => unreachable!(),
        }
    }

    fn store(&self, progress: JobProgress) {
        self.value.store(progress as usize, Ordering::Release);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobResult {
    Valid { size: usize }, // solution size
    Infeasible,
    InvalidInstance,
    EmptySolution,
    SyntaxError,
    SystemError,
    SolverError,
    Timeout,
}

impl JobResult {
    pub fn is_valid(self) -> bool {
        matches!(self, JobResult::Valid { .. })
    }
}

// ToString is more appropriate as we only include partial information
impl Display for JobResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = String::from(match self {
            JobResult::Valid { .. } => "Valid",
            JobResult::Infeasible => "Infeasible",
            JobResult::InvalidInstance => "InvalidInstance",
            JobResult::EmptySolution => "EmptySolution",
            JobResult::SyntaxError => "SyntaxError",
            JobResult::SystemError => "SystemError",
            JobResult::SolverError => "SolverError",
            JobResult::Timeout => "Timeout",
        });
        write!(f, "{}", str)
    }
}

#[derive(Builder)]
pub struct JobProcessor {
    run_directory: Arc<RunDirectory>,
    instance_path: PathBuf,

    solver: PathBuf,
    soft_timeout: Duration,
    grace_period: Duration,

    #[builder(default)]
    solver_args: Vec<String>,

    #[builder(default, setter(skip))]
    progress: AtomicJobProgress,

    #[builder(default)]
    profiler: bool,

    #[builder(default)]
    /// use own binary if omitted
    profiler_executable: Option<PathBuf>,

    #[builder(default)]
    set_stride_envs: bool,
}

impl JobProcessor {
    pub fn instance_path(&self) -> &Path {
        &self.instance_path
    }

    pub fn soft_timeout(&self) -> Duration {
        self.soft_timeout
    }

    pub fn grace_period(&self) -> Duration {
        self.grace_period
    }

    pub fn progress(&self) -> JobProgress {
        self.progress.load()
    }

    pub async fn run(&self) -> (JobResult, Option<SolutionInfos>) {
        let result = self.run_internal().await;
        self.progress.store(JobProgress::Finished);

        match result {
            Ok(r) => r,
            Err(e) => {
                error!("{e}");
                (JobResult::SystemError, None)
            }
        }
    }

    pub async fn run_internal(&self) -> Result<(JobResult, Option<SolutionInfos>), JobError> {
        let work_dir = self
            .run_directory
            .create_task_dir_for(&self.instance_path)?;
        let solution_path = work_dir.join(solver_executor::PATH_STDOUT);

        debug!("JobProcessor {:?} started", self.instance_path);
        // TODO: we might want to avoid the clone of path and arguments ...
        let mut executor_builder = SolverExecutorBuilder::default();

        executor_builder
            .instance_path(self.instance_path.clone())
            .working_dir(work_dir)
            .env(self.env_vars())
            .timeout(self.soft_timeout)
            .grace(self.grace_period);

        if self.profiler {
            // add indirection
            let profiler_path = if let Some(x) = &self.profiler_executable {
                x.clone()
            } else {
                std::env::current_exe().expect("Failed to get current executable path")
            };

            let solver_path = self
                .solver
                .as_os_str()
                .to_str()
                .expect("Convert solver path into String")
                .into();

            let mut args: Vec<String> = vec!["p".into(), solver_path, "--".into()];
            args.extend_from_slice(&self.solver_args);

            executor_builder.solver_path(profiler_path).args(args);
        } else {
            executor_builder
                .solver_path(self.solver.clone())
                .args(self.solver_args.clone());
        }

        let mut executor = executor_builder.build().expect("Executor Builder failed"); // if this fails it is a programming error and will always fail 

        self.progress.store(JobProgress::Running);
        let exit_status = executor.run().await?;
        debug!(
            "JobProcessor {:?} child finished with exit status {:?}. Success: {:?}",
            self.instance_path,
            exit_status,
            exit_status.is_success()
        );

        if !exit_status.is_success() {
            return Ok((
                match exit_status {
                    ChildExitStatus::BeforeTimeout(_) | ChildExitStatus::WithinGrace(_) => {
                        JobResult::SolverError
                    }
                    ChildExitStatus::Timeout => JobResult::Timeout,
                },
                None,
            ));
        }

        self.check_solution(solution_path).await
    }

    async fn check_solution(
        &self,
        solution_path: PathBuf,
    ) -> Result<(JobResult, Option<SolutionInfos>), JobError> {
        self.progress.store(JobProgress::Checking);
        let instance_path = self.instance_path.clone();

        // pace26checker is implemented in a blocking fashion and may also be CPU-bound; so let's move it into an own thread
        let (solution_infos, result) = tokio::task::spawn_blocking(move || {
            let mut checker = CheckAndExtract::new();
            let result = checker.process(&instance_path, &solution_path);
            trace!("[{:?}] CheckAndExtract returned: {result:?}", instance_path);

            let infos = checker.into_solution_infos();

            (infos, result)
        })
        .await?;

        // update solution and map possible error source to job results
        Ok((
            match result {
                Ok(size) => JobResult::Valid { size },
                Err(e) => {
                    error!("{:?} {:?}", self.instance_path, e);
                    map_checker_error_to_job_result(e)
                }
            },
            Some(solution_infos),
        ))
    }

    fn env_vars(&self) -> Vec<(String, String)> {
        if !self.set_stride_envs {
            return Vec::new();
        }

        vec![
            (
                "STRIDE_INSTANCE_PATH".to_string(),
                self.instance_path.to_string_lossy().to_string(),
            ),
            (
                arguments::ENV_SOFT_TIMEOUT.to_string(),
                format!("{}", self.soft_timeout.as_secs_f64()),
            ),
            (
                arguments::ENV_GRACE_PERIOD.to_string(),
                format!("{}", self.grace_period.as_secs_f64()),
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
        CheckerError::EmptySolution => JobResult::EmptySolution,
    }
}
