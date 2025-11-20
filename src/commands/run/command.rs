use std::{fs::File, sync::Arc};

use crate::{
    commands::{arguments::CommandRunArgs, run::instances::*},
    job::job_processor::{JobProcessor, JobProcessorBuilder, JobResult, SolutionInfos},
    run_directory::*,
};
use thiserror::Error;
use tracing::info;

use tokio::time::{Duration, sleep};

#[derive(Error, Debug)]
pub enum CommandRunError {
    #[error(transparent)]
    InstancesError(#[from] InstancesError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

struct RunningTask {
    processor: Arc<JobProcessor>,
    task: tokio::task::JoinHandle<(JobResult, Option<SolutionInfos>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskProgress {
    Running,
    Finished,
}

impl RunningTask {
    fn new(processor: Arc<JobProcessor>) -> Self {
        let moved_processor = processor.clone();
        let task = tokio::spawn(async move { moved_processor.run().await });
        Self { processor, task }
    }

    fn update_progress(&self) -> TaskProgress {
        if self.task.is_finished() {
            TaskProgress::Finished
        } else {
            TaskProgress::Running
        }
    }
}

pub async fn command_run(args: &CommandRunArgs) -> Result<(), CommandRunError> {
    let arc_run_dir = Arc::new(RunDirectory::new()?);

    let log_file = File::create(arc_run_dir.path().join("messages.log"))?;
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(log_file)
        .with_max_level(tracing::Level::TRACE)
        .init();

    let mut instances = {
        let mut instances = Instances::default();
        for p in &args.instances {
            instances.parse_and_insert_path(p)?;
        }
        info!("Found {} instances", instances.len());
        instances.into_iter()
    };

    let parallel_jobs = args.parallel_jobs.unwrap() as usize; // the argument parser ensures this value is always set
    let mut running_tasks = Vec::with_capacity(parallel_jobs);
    loop {
        if running_tasks.len() < parallel_jobs {
            if let Some(instance) = instances.next() {
                // attempt to spawn new task
                let processor = Arc::new(
                    JobProcessorBuilder::default()
                        .run_directory(arc_run_dir.clone())
                        .solver(args.solver.clone())
                        .solver_args(args.solver_args.clone())
                        .soft_timeout(args.soft_timeout)
                        .grace_period(args.grace_period)
                        .instance_path(instance.path().to_path_buf())
                        .build()
                        .unwrap(),
                );

                running_tasks.push(RunningTask::new(processor));
            } else if running_tasks.is_empty() {
                // no running tasks available and no new tasks to spin up
                break;
            }
        }

        running_tasks.retain(|task| task.update_progress() == TaskProgress::Running);

        sleep(Duration::from_millis(1)).await
    }

    Ok(())
}
