use pace26stride::{
    job::job_processor::{JobProcessorBuilder, JobProgress, JobResult},
    run_directory::RunDirectory,
    test_helpers::*,
};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};
use std::{
    path::Path,
    process::{Command, Output, Stdio},
};
use tempdir::TempDir;

fn test_solver_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_test_solver"))
        .canonicalize()
        .unwrap()
}

fn test_stride_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stride"))
        .canonicalize()
        .unwrap()
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum ExpectedResult {
    SuccessRequired,
    FailRequired,
}

#[test]
fn summary() {
    let tempdir = TempDir::new("summary_test").unwrap();

    let list_path = test_testcases_dir()
        .join("test_summary.lst")
        .canonicalize()
        .unwrap();

    run_stride(tempdir.path(), list_path);

    let lines = read_summary(&tempdir.path().join("stride-logs/latest/summary.json"));
    assert_eq!(lines.len(), 10);

    assert_results(&lines);

    // the instance valid_with_info reports #s test_info "there"
    assert_eq!(
        lines
            .get("valid_with_info")
            .unwrap()
            .get("test_info")
            .unwrap()
            .as_str()
            .unwrap(),
        "there"
    );
}

fn assert_results(lines: &HashMap<String, Map<String, Value>>) {
    for (name, expected) in [
        ("syntaxerror", "SyntaxError"),
        ("exit_code1", "SolverError"),
        ("nocover", "SyntaxError"),
        ("valid_alloc50mb", "Valid"),
        ("infeasible", "Infeasible"),
        ("valid", "Valid"),
        ("valid_busywait", "Valid"),
        ("valid_longwait", "Valid"),
        ("valid_wait", "Valid"),
        ("valid_with_info", "Valid"),
    ] {
        let line = lines.get(name).unwrap();

        assert_eq!(
            line.get("s_result").unwrap().as_str().unwrap(),
            expected,
            "entry: {name}"
        );

        if expected == "Valid" {
            assert_eq!(line.get("s_score").unwrap().as_i64().unwrap(), 2);
            assert!(line.contains_key("s_utime"));
            assert!(line.contains_key("s_stime"));
            assert!(line.contains_key("s_wtime"));
            assert!(line.contains_key("s_maxrss"));
            assert!(line.contains_key("s_minflt"));
            assert!(line.contains_key("s_majflt"));
            assert!(line.contains_key("s_nvcsw"));
            assert!(line.contains_key("s_nivcsw"));
        }
    }
}

fn run_stride(tempdir: &Path, list_path: PathBuf) {
    let mut child = Command::new(test_stride_path())
        .current_dir(tempdir)
        .arg("run")
        .arg("--solver")
        .arg(test_solver_path())
        .arg("-i")
        .arg(list_path)
        .arg("--")
        .arg("-f")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let result = child.wait().unwrap();
    assert!(result.success());
}

fn read_summary(path: &Path) -> HashMap<String, Map<String, Value>> {
    let reader = BufReader::new(File::open(path).unwrap());

    let mut values = HashMap::new();

    for line in reader.lines() {
        let line = line.unwrap();
        let content = line.trim();
        if content.is_empty() {
            continue;
        }

        let value: Value = serde_json::from_str(content).unwrap();
        let obj = value.as_object().unwrap();
        let key = obj.get("s_name").unwrap().as_str().unwrap();

        values.insert(key.into(), obj.clone());
    }

    values
}
