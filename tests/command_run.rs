use pace26stride::test_helpers::*;
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};
use std::{
    path::Path,
    process::{Command, Stdio},
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

#[test]
fn no_env() {
    let tempdir = TempDir::new("no_env_test").unwrap();

    let list_path = test_testcases_dir()
        .join("test_solver_valid/report_envs.in")
        .canonicalize()
        .unwrap();

    run_stride(tempdir.path(), list_path, Some(vec!["-E".into()]));
    let lines = read_summary(&tempdir.path().join("stride-logs/latest/summary.json"));

    {
        let envs = lines
            .get("report_envs")
            .unwrap()
            .get("envs")
            .unwrap()
            .as_object()
            .unwrap();

        assert!(!envs.contains_key("STRIDE_INSTANCE_PATH"));
        assert!(!envs.contains_key("STRIDE_TIMEOUT"));
        assert!(!envs.contains_key("STRIDE_GRACE"));
    }
}

#[test]
fn no_profiler() {
    let tempdir = TempDir::new("no_profiler_test").unwrap();

    let list_path = test_testcases_dir()
        .join("test_solver_valid/with_info.in")
        .canonicalize()
        .unwrap();

    run_stride(tempdir.path(), list_path, Some(vec!["-P".into()]));
    let lines = read_summary(&tempdir.path().join("stride-logs/latest/summary.json"));

    {
        let line = lines.get("with_info").unwrap();
        assert!(line.contains_key("test_info"));
        assert!(!line.contains_key("s_utime"));
        assert!(!line.contains_key("s_maxrss"));
    }
}

#[test]
fn multiple_instances() {
    let tempdir = TempDir::new("multiple_instances_test").unwrap();

    let instance_dir = test_testcases_dir()
        .join("test_solver_valid")
        .canonicalize()
        .unwrap();

    let path2 = instance_dir
        .join("with_info.in")
        .to_str()
        .unwrap()
        .to_owned();

    run_stride(
        tempdir.path(),
        instance_dir.join("valid.in"),
        Some(vec![path2]),
    );
    let lines = read_summary(&tempdir.path().join("stride-logs/latest/summary.json"));

    assert_eq!(lines.len(), 2);
}

#[test]
fn summary() {
    let tempdir = TempDir::new("summary_test").unwrap();

    let list_path = test_testcases_dir()
        .join("test_summary.lst")
        .canonicalize()
        .unwrap();

    run_stride(tempdir.path(), list_path, None);

    let lines = read_summary(&tempdir.path().join("stride-logs/latest/summary.json"));
    assert_eq!(lines.len(), 14);

    assert_results(&lines);

    // the instance valid_with_info reports #s test_info "there"
    assert_eq!(
        lines
            .get("with_info")
            .unwrap()
            .get("test_info")
            .unwrap()
            .as_str()
            .unwrap(),
        "there"
    );

    // by default envs are set; make sure they are there!
    {
        let envs = lines
            .get("report_envs")
            .unwrap()
            .get("envs")
            .unwrap()
            .as_object()
            .unwrap();

        assert!(envs.contains_key("STRIDE_INSTANCE_PATH"));
        assert!(envs.contains_key("STRIDE_TIMEOUT"));
        assert!(envs.contains_key("STRIDE_GRACE"));
    }
}

#[test]
fn relative_solver_path() {
    let tempdir = TempDir::new("relative_solver").unwrap();
    std::fs::copy(test_solver_path(), tempdir.path().join("solver")).unwrap();

    let list_path = test_testcases_dir()
        .join("test_solver_valid/with_info.in")
        .canonicalize()
        .unwrap();

    let exec_path = test_stride_path();

    let args = vec![
        String::from("run"),
        String::from("--solver"),
        String::from("solver"),
        String::from("-i"),
        list_path.to_str().unwrap().to_owned(),
    ];

    println!(
        "Run {} {} in {}",
        exec_path.display(),
        args.join(" "),
        tempdir.path().display()
    );

    let mut command = Command::new(exec_path);
    let output = command
        .current_dir(tempdir.path())
        .args(args)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    println!("{stdout}, {stderr}");

    assert!(stdout.contains("'./'") || stderr.contains("'./'"));
}

fn assert_results(lines: &HashMap<String, Map<String, Value>>) {
    for (name, expected) in [
        ("syntaxerror", "SyntaxError"),
        ("exit_code1", "SolverError"),
        ("nocover", "SyntaxError"),
        ("alloc50mb", "Valid"),
        ("infeasible", "Infeasible"),
        ("timeout", "Timeout"),
        ("valid", "Valid"),
        ("requires_grace", "Valid"),
        ("busywait", "Valid"),
        ("idlewait", "Valid"),
        ("shortwait", "Valid"),
        ("with_info", "Valid"),
        ("report_envs", "Valid"),
    ] {
        assert!(lines.contains_key(name), "missing: {name}");
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

fn run_stride(tempdir: &Path, list_path: PathBuf, stride_args: Option<Vec<String>>) {
    let exec_path = test_stride_path();

    let mut args: Vec<String> = vec![
        "run".into(),
        "--solver".into(),
        test_solver_path().to_string_lossy().to_string(),
        "-t".into(),
        "2".into(),
        "-g".into(),
        "1".into(),
        "-i".into(),
        list_path.to_string_lossy().to_string(),
    ];

    if let Some(mut sargs) = stride_args {
        args.append(&mut sargs);
    }

    args.push("--".into());
    args.push("-f".into());

    println!("Exec {} {}", exec_path.display(), args.join(" "));

    let mut child = Command::new(exec_path)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(tempdir)
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
        let key = obj.get("s_key").unwrap().as_str().unwrap();

        values.insert(key.into(), obj.clone());
    }

    values
}
