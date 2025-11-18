// Semiintegration test of job::job_processor. While most logic seems a good fit for
// a unit test, we implement it as an unit test because, we need the test_solver binary
// fully build.

use pace26stride::{
    commands::arguments::CommandRunArgs,
    job::job_processor::{JobProcessor, JobProgress},
    run_directory::RunDirectory,
    test_helpers::*,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tempdir::TempDir;

fn test_solver_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_test_solver"))
}

fn default_opts() -> CommandRunArgs {
    let mut arguments = CommandRunArgs::default();
    arguments.soft_timeout = Duration::from_secs(1);
    arguments.grace_period = Duration::from_secs(1);
    arguments.solver = test_solver_path();
    arguments
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum ExpectedResult {
    SuccessRequired,
    FailRequired,
}

async fn test_solutions(arguments: CommandRunArgs, key: &str, expected: ExpectedResult) {
    let instances = test_cases_glob(key);

    let tempdir = TempDir::new(key).unwrap();
    let run_dir = Arc::new(RunDirectory::new_within(tempdir.path()).unwrap());

    let arguments = Arc::new(arguments);

    let mut handles = Vec::new();
    for instance_path in instances {
        let run_dir = run_dir.clone();
        let arguments = arguments.clone();
        handles.push(tokio::spawn(async move {
            let mut job = JobProcessor::new(run_dir, arguments, instance_path.clone());
            let result = job.run().await;
            assert!(result.is_ok());
            assert_eq!(job.progress(), JobProgress::Finished);

            let job_result = job.result().unwrap();
            assert_eq!(
                job_result.is_valid(),
                expected == ExpectedResult::SuccessRequired,
                "{instance_path:?}: {job_result:?}"
            );
        }));
    }

    assert!(!handles.is_empty());

    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_valid_solutions() {
    test_solutions(
        default_opts(),
        "valid_solutions",
        ExpectedResult::SuccessRequired,
    )
    .await
}

#[tokio::test]
async fn test_invalid_solutions() {
    test_solutions(
        default_opts(),
        "invalid_solutions",
        ExpectedResult::FailRequired,
    )
    .await
}
