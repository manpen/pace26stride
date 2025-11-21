use crate::{
    commands::{
        arguments::CommandRunArgs,
        run::{
            display::{JobProgressBar, ProgressDisplay},
            instances::*,
        },
    },
    job::job_processor::{JobProcessor, JobProcessorBuilder, JobResult, SolutionInfos},
    run_directory::*,
};
use std::{fs::File, sync::Arc};
use thiserror::Error;
use tracing::{debug, info};

use tokio::{
    task::block_in_place,
    time::{Duration, sleep},
};

#[derive(Error, Debug)]
pub enum CommandRunError {
    #[error(transparent)]
    InstancesError(#[from] InstancesError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type TaskResult = (JobResult, Option<SolutionInfos>);
struct RunningTask {
    processor: Arc<JobProcessor>,
    job_progress_bar: JobProgressBar,
    task: tokio::task::JoinHandle<TaskResult>,
}

impl RunningTask {
    fn new(processor: Arc<JobProcessor>) -> Self {
        let moved_processor = processor.clone();
        let task = tokio::spawn(async move { moved_processor.run().await });
        let job_progress_bar = JobProgressBar::new(
            String::from(
                processor
                    .instance_path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unnamed"),
            ),
            processor.soft_timeout(),
            processor.grace_period(),
        );

        Self {
            processor,
            job_progress_bar,
            task,
        }
    }

    fn is_finished(&mut self, display: &ProgressDisplay, now: tokio::time::Instant) -> bool {
        let progress = self.processor.progress();
        self.job_progress_bar
            .update_progress_bar(display, progress, now);

        self.task.is_finished()
    }

    fn finish(self, display: &mut ProgressDisplay) -> TaskResult {
        debug!("{:?} block_on task", self.processor.instance_path());
        let result = block_in_place(|| futures::executor::block_on(self.task)).unwrap();
        self.job_progress_bar.finish(display, result.0);
        result
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

    let mut display = ProgressDisplay::new(instances.len());

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

        let now = tokio::time::Instant::now();

        {
            let mut idx = 0;
            while idx < running_tasks.len() {
                if running_tasks[idx].is_finished(&display, now) {
                    // task finished
                    let _ = running_tasks.swap_remove(idx).finish(&mut display);
                } else {
                    idx += 1;
                }
            }
        }

        display.tick(running_tasks.len());

        sleep(Duration::from_millis(1)).await
    }

    display.final_message();

    Ok(())
}
