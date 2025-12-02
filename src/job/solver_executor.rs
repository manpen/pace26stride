use std::{fs::File, io::Write, path::PathBuf, process::ExitStatus, time::Duration};

use derive_builder::Builder;
use thiserror::Error;
use tokio::{
    process::{Child, Command},
    time::{Instant, timeout},
};
use tracing::{debug, trace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildExitStatus {
    BeforeTimeout(ExitStatus),
    WithinGrace(ExitStatus),
    Timeout,
}

impl ChildExitStatus {
    pub fn is_success(self) -> bool {
        match self {
            ChildExitStatus::BeforeTimeout(exit_status) => exit_status.success(),
            ChildExitStatus::WithinGrace(exit_status) => exit_status.success(),
            ChildExitStatus::Timeout => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Timeout error: {0}")]
    Timeout(#[from] tokio::time::error::Elapsed),
}

#[derive(Debug, Builder)]
pub struct SolverExecutor {
    instance_path: PathBuf,
    working_dir: PathBuf,
    solver_path: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,

    timeout: Duration,
    grace: Duration,

    #[builder(default)]
    runtime: Option<Duration>,
}

pub const PATH_STDOUT: &str = "stdout";
pub const PATH_STDERR: &str = "stderr";

impl SolverExecutor {
    pub async fn run(&mut self) -> Result<ChildExitStatus, ExecutorError> {
        // spawn and execute solver as child
        let start_time = Instant::now();
        let child = self.spawn_child()?;
        let wait_result = self.timeout_wait_for_child_to_complete(child).await?;
        self.runtime = Some(start_time.elapsed());

        Ok(wait_result)
    }

    fn spawn_child(&mut self) -> Result<Child, ExecutorError> {
        let stdin = File::open(&self.instance_path)?;
        let mut stdout = File::create(self.working_dir.join(PATH_STDOUT))?;
        let stderr = File::create(self.working_dir.join(PATH_STDERR))?;

        if let Some(solver) = self.solver_path.as_os_str().to_str() {
            let _ = writeln!(stdout, "# cmd:  {} {}", solver, self.args.join(" "));
        }

        if let Some(instance) = self.instance_path.as_os_str().to_str() {
            let _ = writeln!(stdout, "# instance: {}", instance);
        }

        trace!(
            "Spawn solver {:?} with args {:?}",
            self.solver_path, &self.args
        );

        let child = Command::new(&self.solver_path)
            .args(&self.args)
            .envs(self.env.iter().cloned())
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(true)
            .spawn()?;

        Ok(child)
    }

    /// In case of no error, we return
    ///  - Some(ExitStatus) if the child has exited
    ///  - None if the child has been killed using SIGKILL
    async fn timeout_wait_for_child_to_complete(
        &self,
        mut child: Child,
    ) -> Result<ChildExitStatus, ExecutorError> {
        // we get an error if we run into the timeout
        if let Ok(res) = timeout(self.timeout, child.wait()).await {
            return Ok(ChildExitStatus::BeforeTimeout(res?));
        }

        debug!(
            "[{:?}] Timeout after {}s reached; send sigterm child",
            self.instance_path,
            self.timeout.as_secs()
        );

        // send SIGTERM to the child (we use unsafe here, because I do not want to pull a crate for this one line)
        if let Some(pid) = child.id() {
            // we only get None if the child has already exited
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }

        // issue a grace period
        if !self.grace.is_zero()
            && let Ok(res) = timeout(self.grace, child.wait()).await
        {
            return Ok(ChildExitStatus::WithinGrace(res?));
        }

        debug!(
            "[{:?}] Grace period after {}s reached; kill child",
            self.instance_path,
            self.timeout.as_secs()
        );

        child.kill().await?;

        Ok(ChildExitStatus::Timeout)
    }
}
