use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
};

fn testcase_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testcases")
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_stride"))
}

#[test]
fn dot_solutions() {
    let testcases_path = testcase_dir().join("valid_solutions").join("*.in");
    let instance_path = glob::glob(testcases_path.as_os_str().to_str().unwrap()).unwrap();

    let mut num_success = 0;
    for (i, input_path) in instance_path.enumerate() {
        let input_path = input_path.unwrap();
        let mut output_path = input_path.clone();
        assert!(output_path.set_extension("out"));
        assert!(output_path.exists());

        let mut args = vec![
            String::from("check"),
            String::from("-d"),
            input_path.as_os_str().to_str().unwrap().into(),
        ];

        if i % 2 == 1 {
            args.push(output_path.as_os_str().to_str().unwrap().into());
        }

        let output = command().args(args).output().expect("failed to run binary");

        assert!(output.status.success());

        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("digraph"));

        num_success += 1;
    }

    assert!(num_success > 10);
}

#[test]
fn valid_solutions() {
    let testcases_path = testcase_dir().join("valid_solutions").join("*.in");
    let instance_path = glob::glob(testcases_path.as_os_str().to_str().unwrap()).unwrap();

    let path_re = regex::Regex::new(r"score(\d+)_").unwrap();
    let solution_re = regex::bytes::Regex::new(r"#s solution_size \s*(\d+)\n").unwrap();

    let mut num_success = 0;
    for input_path in instance_path {
        let input_path = input_path.unwrap();
        let mut output_path = input_path.clone();
        assert!(output_path.set_extension("out"));
        assert!(output_path.exists());

        let trees_according_to_filename: usize = {
            let stem = input_path.file_stem().unwrap().to_str().unwrap();
            let captures = path_re.captures(stem).expect("testcases in the good folder need to start with `scoreXX_` where XX is the number of trees contained");
            captures[1].parse().unwrap()
        };

        let output = command()
            .arg("check")
            .arg(input_path)
            .arg(output_path)
            .output()
            .expect("failed to run binary");

        assert!(output.status.success());

        let trees_according_to_checker: usize = {
            let captures = solution_re
                .captures(&output.stdout)
                .expect("Solution size not found in stdout");
            String::from_utf8(captures[1].to_vec())
                .unwrap()
                .parse()
                .unwrap()
        };

        assert_eq!(trees_according_to_filename, trees_according_to_checker);

        num_success += 1;
    }

    assert!(num_success > 10);
}

#[test]
fn invalid_cases() {
    let testcases_path = testcase_dir().join("i*").join("*.in");
    let instance_path = glob::glob(testcases_path.as_os_str().to_str().unwrap()).unwrap();

    for input_path in instance_path {
        let mut args = Vec::new();

        // all tests have an instance file
        let input_path = input_path.unwrap();
        args.push(input_path.clone());

        // some tests do not have a solution file
        {
            let mut output_path = input_path.clone();
            assert!(output_path.set_extension("out"));
            if output_path.exists() {
                args.push(output_path);
            }
        }

        // run binary and make sure it report non-success
        let output = command()
            .arg("check")
            .args(args)
            .output()
            .expect("failed to run binary");
        assert!(!output.status.success());

        let reader = BufReader::new(File::open(&input_path).expect("Open instance file"));
        let patterns: Vec<_> = reader
            .lines()
            .filter_map(|l| l.ok())
            .filter_map(|l| Some(l.strip_prefix("# REQUIRE: ")?.to_owned()))
            .collect();

        assert!(
            !patterns.is_empty(),
            "At least one # REQUIRE: line expected"
        );

        for pattern in patterns {
            let re = regex::bytes::Regex::new(&pattern).expect("Valid pattern");
            assert!(
                re.find(&output.stderr).is_some(),
                "Pattern not found: {pattern}. Found: {}\ninput_path: {input_path:?}",
                String::from_utf8(output.stderr).unwrap()
            );
        }
    }
}

#[test]
fn digest() {
    let instance_path = testcase_dir()
        .join("valid_solutions")
        .join("score10_n07l_lkc.in");
    let solution_path = testcase_dir()
        .join("valid_solutions")
        .join("score10_n07l_lkc.out");

    const IDIGEST: &str = "015e2a313c3470afe151d1ac2a20b0f3";
    const SDIGEST: &str = "000a5d234052384b8fd9ea7bac0d3c17";

    for with_solution in [false, true] {
        let mut args = vec![
            String::from("check"),
            String::from("-H"),
            instance_path.as_os_str().to_str().unwrap().into(),
        ];

        if with_solution {
            args.push(solution_path.as_os_str().to_str().unwrap().into());
        }

        let output = command().args(args).output().expect("failed to run binary");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();

        assert!(stdout.contains(format!("#s idigest \"{IDIGEST}\"").as_str()));

        if with_solution {
            assert!(stdout.contains(format!("#s sdigest \"{SDIGEST}\"").as_str()));
        }
    }
}
