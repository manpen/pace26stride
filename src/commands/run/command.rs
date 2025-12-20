use crate::{
    commands::{
        arguments::CommandRunArgs,
        run::{
            display::{JobProgressBar, ProgressDisplay},
            instances::*,
            summary_writer::SummaryWriter,
        },
    },
    job::job_processor::{JobProcessorBuilder, JobResult},
    run_directory::*,
};
use std::collections::hash_set::IntoIter;
use std::path::PathBuf;
use std::{fs::File, sync::Arc};
use thiserror::Error;
use tracing::{error, info, trace};

use crate::commands::run::upload::{JobResultUploadAggregation, UploadToStride};
use crate::job::check_and_extract::SolutionInfos;
use pace26checker::digest::digest_output::InstanceDigest;
use pace26remote::job_description;
use pace26remote::job_description::JobDescription;
use pace26remote::upload::UploadError;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;
use tokio::time::{Duration, sleep};

const DISPLAY_TICK_MIN_WAIT: Duration = Duration::from_millis(25);

pub async fn command_run(args: &CommandRunArgs) -> Result<(), CommandRunError> {
    let mut task_context = TaskContext::new(args.clone()).await?;

    initialize_logger(&task_context)?;
    let (mut instances, instances_with_digest) = collect_instances(&args.instances)?;
    task_context.display.set_total_instance(instances.len());
    if !args.offline && instances_with_digest > 0 {
        task_context.enable_uploader()?;
        task_context
            .display
            .set_num_stride_instance(instances_with_digest);
    }

    let task_context = Arc::new(task_context);

    // We will spawn upto `num_parallel_jobs` in parallel. This rate limit is enforced using the
    // Semaphore `parallel_jobs_sema`. Each task gets sequenced using an own Tokio task, spawned
    // from `task_main`. We pass the semaphore's permit into this task, in general, the task
    // may live much longer than the solver. For instance, the task also handles communication
    // with the stride server and writing into the summary.
    let num_parallel_jobs = args.parallel_jobs.unwrap() as usize;
    let parallel_jobs_sema = Arc::new(Semaphore::new(num_parallel_jobs));
    let mut join_handles = Vec::with_capacity((100 * num_parallel_jobs).min(instances.len()));

    loop {
        if let Ok(permit) = timeout(
            DISPLAY_TICK_MIN_WAIT,
            parallel_jobs_sema.clone().acquire_owned(),
        )
        .await
        {
            let Some(instance) = instances.next() else {
                break;
            };

            if let Ok(permit) = permit {
                join_handles.push(tokio::spawn(task_main(
                    task_context.clone(),
                    instance,
                    permit,
                )));
            } else {
                error!("Semaphore closed");
                break;
            }
        }

        join_handles.retain(|h| !h.is_finished());
        task_context
            .display
            .tick(num_parallel_jobs - parallel_jobs_sema.available_permits());
    }

    // at this point, no instance remain to be started, but some solvers can run
    while parallel_jobs_sema.available_permits() < num_parallel_jobs {
        task_context
            .display
            .tick(num_parallel_jobs - parallel_jobs_sema.available_permits());

        sleep(DISPLAY_TICK_MIN_WAIT).await;
    }

    task_context.display.switch_to_postprocessing();

    for mut h in join_handles {
        loop {
            task_context.display.post_processing_tick();
            if timeout(DISPLAY_TICK_MIN_WAIT, &mut h).await.is_ok() {
                break;
            }
        }
    }

    sleep(DISPLAY_TICK_MIN_WAIT).await;
    task_context.display.post_processing_tick();
    task_context.display.final_message();

    Ok(())
}

#[derive(Error, Debug)]
pub enum CommandRunError {
    #[error(transparent)]
    InstancesError(#[from] InstancesError),

    #[error(transparent)]
    InstanceDir(#[from] CreateInstanceDirError),

    #[error(transparent)]
    UploadError(#[from] UploadError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

struct TaskContext {
    args: CommandRunArgs,
    display: ProgressDisplay,
    run_dir: Arc<RunDirectory>,
    uploader: Option<JobResultUploadAggregation>,
    summary_writer: SummaryWriter,
}

impl TaskContext {
    async fn new(args: CommandRunArgs) -> Result<Self, CommandRunError> {
        let run_dir = RunDirectory::new()?;

        let display = ProgressDisplay::new(0);

        let summary_writer = SummaryWriter::new(&run_dir.path().join("summary.json")).await?;

        Ok(Self {
            args,
            display,
            run_dir: Arc::new(run_dir),
            uploader: None,
            summary_writer,
        })
    }

    fn enable_uploader(&mut self) -> Result<(), CommandRunError> {
        assert!(self.uploader.is_none());

        let uploader = Arc::new(UploadToStride::new_with_server(
            self.args.solution_server.clone(),
        )?);

        self.uploader = Some(JobResultUploadAggregation::new(uploader));

        Ok(())
    }
}

async fn task_main(
    context: Arc<TaskContext>,
    instance: Instance,
    permit: OwnedSemaphorePermit,
) -> Result<(), CommandRunError> {
    let work_dir = context
        .run_dir
        .create_task_dir_for(&PathBuf::from(instance.name()))?;

    let processor = Arc::new(
        JobProcessorBuilder::default()
            .work_dir(work_dir.clone())
            .solver(context.args.solver.clone())
            .solver_args(context.args.solver_args.clone())
            .soft_timeout(context.args.soft_timeout)
            .grace_period(context.args.grace_period)
            .instance_path(instance.path().to_path_buf())
            .profiler(!context.args.no_profile)
            .set_stride_envs(!context.args.no_envs)
            .build()
            .unwrap(),
    );

    let task = {
        let processor = processor.clone();
        tokio::spawn(async move { processor.run().await })
    };

    let mut job_progress_bar = JobProgressBar::new(
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

    while !task.is_finished() {
        let progress = processor.progress();
        job_progress_bar.update_progress_bar(&context.display, progress);

        sleep(DISPLAY_TICK_MIN_WAIT).await;
    }

    // we only reach this point, if the task finished; so awaiting it should be fast
    let (job_result, mut opt_info) = task.await.unwrap();
    job_progress_bar.finish(&context.display, job_result);

    // all remaining steps require very little compute -- so we drop the rate limit permit
    // to free the resources needed for a new solver run
    drop(permit);

    let mut keep_work_dir = context.args.keep_successful_logs;
    keep_work_dir |= !job_result.is_valid();

    // upload and fetch best known
    let upload_desc = if !context.args.offline
        && let Some(idigest) = instance.idigest()
    {
        let runtime = processor.runtime().expect("failed to get runtime"); // runtime will always be set if the child terminated, independently of successes
        prepare_upload_descriptor(idigest, runtime, job_result, &mut opt_info)
    } else {
        None
    };

    let score = if let Some(desc) = &upload_desc
        && let job_description::JobResult::Valid { score, .. } = desc.result
    {
        context.display.stride_inc_queued();
        Some(score)
    } else {
        None
    };

    let best_known = if let Some(uploader) = context.uploader.as_ref()
        && let Some(desc) = upload_desc
    {
        let response = uploader.upload_and_fetch_best_known(desc).await;
        let score = score.unwrap();

        if let Some(best_known) = response {
            if best_known > score {
                context.display.stride_new_best_known();
            } else if best_known == score {
                context.display.stride_inc_best_known();
            } else {
                context.display.stride_suboptimal();
                keep_work_dir |= context.args.require_optimal;
            }
        } else {
            context.display.stride_inc_no_response();
        }

        response
    } else {
        None
    };

    if let Err(e) = context
        .summary_writer
        .add_entry(&instance, job_result, opt_info, best_known)
        .await
    {
        error!("SummaryWriter error: {e:?}");
    }

    if keep_work_dir {
        let group = job_result.to_string().to_lowercase();
        let parent = context.run_dir.path().join(group.as_str());
        let target = parent.join(instance.name());
        trace!(
            "Move workdir {} to {}",
            work_dir.display(),
            target.display()
        );
        tokio::fs::create_dir_all(&parent).await?;
        tokio::fs::rename(work_dir, &target).await?;
        tokio::fs::symlink(instance.path().canonicalize()?, target.join("stdin")).await?;
    } else {
        trace!("Remove workdir {}", work_dir.display());
        tokio::fs::remove_dir_all(work_dir).await?;
    }

    Ok(())
}

fn prepare_upload_descriptor(
    idigest: InstanceDigest,
    runtime: Duration,
    job_result: JobResult,
    opt_info: &mut Option<SolutionInfos>,
) -> Option<JobDescription> {
    match job_result {
        JobResult::Valid { .. } => {
            if let Some(opt_info) = opt_info {
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
        JobResult::Infeasible => Some(JobDescription::infeasible(idigest, Some(runtime))),
        JobResult::Timeout => Some(JobDescription::timeout(idigest, runtime)),
        _ => None,
    }
}

fn collect_instances(
    args_instances: &[PathBuf],
) -> Result<(IntoIter<Instance>, usize), CommandRunError> {
    let mut instances = Instances::default();
    for p in args_instances {
        instances.parse_and_insert_path(p)?;
    }
    let instances_with_digest = instances.iter().filter_map(|i| i.idigest()).count();
    info!(
        "Found {} instances. Of those {} have an idigest",
        instances.len(),
        instances_with_digest
    );

    Ok((instances.into_iter(), instances_with_digest))
}

fn initialize_logger(task_context: &TaskContext) -> Result<(), CommandRunError> {
    let log_file = File::create(task_context.run_dir.path().join("messages.log"))?;
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(log_file)
        .with_max_level(tracing::Level::TRACE)
        .init();
    Ok(())
}
