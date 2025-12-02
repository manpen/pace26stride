use pace26stride::{
    job::job_processor::{JobProcessorBuilder, JobResult},
    run_directory::RunDirectory,
    test_helpers::*,
};
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tempdir::TempDir;

fn test_solver_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_test_solver"))
}

fn test_stride_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stride"))
}

async fn run(instance: PathBuf, profiler: bool) -> (JobResult, HashMap<String, Value>) {
    let instance = test_testcases_dir().join(instance);
    let tempdir = TempDir::new("profile_test").unwrap();
    let run_dir = Arc::new(RunDirectory::new_within(tempdir.path()).unwrap());

    let job = JobProcessorBuilder::default()
        .soft_timeout(Duration::from_secs_f64(1.5))
        .grace_period(Duration::from_secs_f64(1.5))
        .solver(test_solver_path())
        .solver_args(vec!["-f".into()])
        .run_directory(run_dir)
        .instance_path(instance)
        .profiler(profiler)
        .profiler_executable(Some(test_stride_path()))
        .build()
        .unwrap();

    let (job_result, solution_infos) = job.run().await;

    let mut infos = HashMap::new();
    if let Some(vec) = solution_infos {
        for (key, value) in vec {
            infos.insert(key, value);
        }
    }

    (job_result, infos)
}

#[tokio::test]
async fn valid_wo_profiler() {
    let (result, _infos) = run(PathBuf::from("test_solver_valid/valid.in"), false).await;
    assert_eq!(result, JobResult::Valid { size: 2 });
}

#[tokio::test]
async fn valid_with_profiler() {
    let (result, _infos) = run(PathBuf::from("test_solver_valid/valid.in"), true).await;
    assert_eq!(result, JobResult::Valid { size: 2 });
}

#[tokio::test]
async fn profile_time() {
    // idle wait
    let (result, infos) = run(PathBuf::from("test_solver_valid/valid_wait.in"), true).await;
    assert_eq!(result, JobResult::Valid { size: 2 });

    assert!(infos.get("s_wtime").unwrap().as_f64().unwrap() > 0.7);
    assert!(infos.get("s_utime").unwrap().as_f64().unwrap() < 0.5);

    // busy wait
    let (result, infos) = run(PathBuf::from("test_solver_valid/valid_busywait.in"), true).await;
    assert_eq!(result, JobResult::Valid { size: 2 });

    assert!(infos.get("s_wtime").unwrap().as_f64().unwrap() > 0.7);
    assert!(infos.get("s_utime").unwrap().as_f64().unwrap() > 0.7);
}

#[tokio::test]
async fn profile_maxrss() {
    // idle wait
    let (result, infos) = run(PathBuf::from("test_solver_valid/valid.in"), true).await;
    assert_eq!(result, JobResult::Valid { size: 2 });
    let maxrss_before = infos.get("s_maxrss").unwrap().as_i64().unwrap();

    // busy wait
    let (result, infos) = run(PathBuf::from("test_solver_valid/valid_alloc50mb.in"), true).await;
    assert_eq!(result, JobResult::Valid { size: 2 });
    let maxrss_after = infos.get("s_maxrss").unwrap().as_i64().unwrap();

    // make sure it's atleast 30mb larger
    assert!(maxrss_before + 30_000_000 < maxrss_after);
}
