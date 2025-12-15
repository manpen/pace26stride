use crate::{
    commands::{
        arguments::CommandRunArgs,
        run::{
            display::{JobProgressBar, ProgressDisplay},
            instances::*,
            summary_writer::SummaryWriter,
        },
    },
    job::job_processor::{JobProcessor, JobProcessorBuilder, JobResult},
    run_directory::*,
};
use std::{fs::File, sync::Arc};
use thiserror::Error;
use tracing::{debug, error, info};

use crate::commands::run::upload::{JobResultUploadAggregation, UploadToStride};
use crate::job::check_and_extract::SolutionInfos;
use pace26remote::job_description::JobDescription;
use pace26remote::upload::UploadError;
use tokio::{
    task::block_in_place,
    time::{Duration, sleep},
};
use tokio::time::timeout;

#[derive(Error, Debug)]
pub enum CommandRunError {
    #[error(transparent)]
    InstancesError(#[from] InstancesError),

    #[error(transparent)]
    UploadError(#[from] UploadError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type TaskResult = (JobResult, Option<SolutionInfos>);
struct RunningTask {
    instance: Instance,
    processor: Arc<JobProcessor>,
    job_progress_bar: JobProgressBar,
    task: tokio::task::JoinHandle<TaskResult>,
}

impl RunningTask {
    fn new(instance: Instance, processor: Arc<JobProcessor>) -> Self {
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
            instance,
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

    fn finish(self, display: &mut ProgressDisplay) -> (Instance, TaskResult) {
        debug!("{:?} block_on task", self.processor.instance_path());
        let result = block_in_place(|| futures::executor::block_on(self.task)).unwrap();
        self.job_progress_bar.finish(display, result.0);
        (self.instance, result)
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

    let (mut instances, instances_with_digest) = {
        let mut instances = Instances::default();
        for p in &args.instances {
            instances.parse_and_insert_path(p)?;
        }
        let instances_with_digest = instances.iter().filter_map(|i| i.idigest()).count();
        info!(
            "Found {} instances. Of those {} have an idigest",
            instances.len(),
            instances_with_digest
        );

        (instances.into_iter(), instances_with_digest)
    };

    let upload_aggr = if instances_with_digest > 0 && !args.offline {
        let uploader = Arc::new(UploadToStride::new_with_server(
            args.solution_server.clone(),
        )?);
        Some(Arc::new(JobResultUploadAggregation::new(uploader)))
    } else {
        None
    };

    let mut display = ProgressDisplay::new(instances.len());

    let summary_writer =
        Arc::new(SummaryWriter::new(&arc_run_dir.path().join("summary.json")).await?);

    let parallel_jobs = args.parallel_jobs.unwrap() as usize; // the argument parser ensures this value is always set
    let mut running_tasks = Vec::with_capacity(parallel_jobs);
    let mut join_handles = Vec::with_capacity(parallel_jobs);
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
                        .profiler(!args.no_profile)
                        .set_stride_envs(!args.no_envs)
                        .build()
                        .unwrap(),
                );

                running_tasks.push(RunningTask::new(instance, processor));
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
                    let (instance, (job_result, mut opt_info)) =
                        running_tasks.swap_remove(idx).finish(&mut display);

                    let upload_desc = if !args.offline
                        && let Some(idigest) = instance.idigest()
                    {
                        let runtime = Duration::from_millis(123);
                        match job_result {
                            JobResult::Valid { .. } => {
                                if let Some(opt_info) = &mut opt_info {
                                    let mut trees = std::mem::take(&mut opt_info.0);
                                    Some(JobDescription::valid_from_strings(
                                        idigest,
                                        &mut trees,
                                        Some(runtime),
                                    ))
                                } else {
                                    None
                                }
                            }
                            JobResult::Infeasible => {
                                Some(JobDescription::infeasible(idigest, Some(runtime)))
                            }
                            JobResult::Timeout => Some(JobDescription::timeout(idigest, runtime)),
                            _ => None,
                        }
                    } else {
                        None
                    };

                    if let Some(upload_aggr) = upload_aggr.as_ref()
                        && let Some(desc) = upload_desc
                    {
                        let upload_aggr = upload_aggr.clone();
                        let summary_writer = summary_writer.clone();
                        join_handles.push(tokio::spawn(async move {
                            let result = upload_aggr.upload_and_fetch_best_known(desc).await;
                            if let Err(e) = summary_writer
                                .add_entry(&instance, job_result, opt_info, result)
                                .await
                            {
                                error!("SummaryWriter error: {e:?}");
                            }
                        }));
                    } else if let Err(e) = summary_writer
                        .add_entry(&instance, job_result, opt_info, None)
                        .await
                    {
                        error!("SummaryWriter error: {e:?}");
                    }
                } else {
                    idx += 1;
                }
            }
        }

        join_handles.retain(|h| !h.is_finished());

        display.tick(running_tasks.len());

        sleep(Duration::from_millis(5)).await
    }

    display.switch_to_postprocessing();

    for mut h in join_handles {
        loop {
            display.post_processing_tick();
            if timeout(Duration::from_millis(20), &mut h).await.is_ok() {
                break;
            }
        }
    }

    display.final_message();

    Ok(())
}
