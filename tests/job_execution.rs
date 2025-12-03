// Semiintegration test of job::job_processor. While most logic seems a good fit for
// a unit test, we implement it as an unit test because, we need the test_solver binary
// fully build.

use pace26stride::{
    job::job_processor::{JobProcessorBuilder, JobProgress},
    run_directory::RunDirectory,
    test_helpers::*,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tempdir::TempDir;

fn test_solver_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_test_solver"))
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum ExpectedResult {
    SuccessRequired,
    FailRequired,
}

async fn test_solutions(key: &str, expected: ExpectedResult) {
    let instances = test_cases_glob(key);

    let tempdir = TempDir::new(key).unwrap();
    let run_dir = Arc::new(RunDirectory::new_within(tempdir.path()).unwrap());

    let mut handles = Vec::new();
    for instance_path in instances {
        let run_dir = run_dir.clone();
        handles.push(tokio::spawn(async move {
            let job = JobProcessorBuilder::default()
                .soft_timeout(Duration::from_secs(1))
                .grace_period(Duration::from_secs(1))
                .solver(test_solver_path())
                .run_directory(run_dir)
                .instance_path(instance_path.clone())
                .set_stride_envs(true)
                .build()
                .unwrap();

            let (job_result, _solution_infos) = job.run().await;
            assert_eq!(job.progress(), JobProgress::Finished);

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
    test_solutions("valid_solutions", ExpectedResult::SuccessRequired).await
}

#[tokio::test]
async fn test_invalid_solutions() {
    test_solutions("invalid_solutions", ExpectedResult::FailRequired).await
}
