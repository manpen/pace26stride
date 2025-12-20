use clap::Parser;
use serde::Deserialize;
/// This binary is used to test the runner.
///  Standard behavior is to print out the contents of the file "{X}.out" where X is the value of the environment variable "STRIDE_INSTANCE_PATH".
///  It also supports additional options to wait, ignore SIGTERM, and set exit code.
use std::{
    collections::HashMap,
    hint::black_box,
    io::{BufRead, stdin},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

const PARAM_LINE_PREFIX: &str = "#s test_params ";

#[derive(Parser, Deserialize)]
struct Opts {
    #[arg(
        short,
        long,
        help = "Number of seconds to wait before output",
        default_value = "0"
    )]
    #[serde(default)]
    wait_seconds: f64,

    #[arg(
        short,
        long,
        help = "Number of seconds to busy wait before output",
        default_value = "0"
    )]
    #[serde(default)]
    busy_wait_seconds: f64,

    #[arg(short = 'm', long, help = "Allocate extra memory (in bytes)")]
    #[serde(default)]
    extra_alloc: Option<usize>,

    #[arg(short, long, help = "Ignore SIGTERM signal")]
    #[serde(default)]
    ignore_sigterm: bool,

    #[arg(short = 'e', long, help = "Exit code to return", default_value = "0")]
    #[serde(default)]
    exit_code: i32,

    #[arg(short = 'f', long, help = "Read settings from STDIN")]
    #[serde(default)]
    from_stdin: bool,

    #[arg(short = 'p', long, help = "Print string instead of solution")]
    #[serde(default)]
    print: Option<String>,

    #[arg(short = 'E', long, help = "Report environment variable")]
    #[serde(default)]
    report_environment: bool,
}

fn parse_opts_from_stdin() -> Option<Opts> {
    for line in stdin().lock().lines() {
        if let Ok(line) = line
            && line.starts_with(PARAM_LINE_PREFIX)
        {
            let value = line.as_str().strip_prefix(PARAM_LINE_PREFIX).unwrap();
            return Some(serde_json::from_str(value).unwrap());
        }
    }
    None
}

fn main() {
    let opts = Opts::parse();

    let signal_received = Arc::new(AtomicBool::new(false));

    let opts = if opts.from_stdin {
        parse_opts_from_stdin().unwrap()
    } else {
        opts
    };

    if opts.report_environment {
        let vars: HashMap<_, _> = std::env::vars().collect();
        println!("#s envs {}", serde_json::to_string(&vars).unwrap());
    }

    {
        let signal_received_clone = signal_received.clone();
        ctrlc::set_handler(move || {
            println!("#s s_sigterm true");
            signal_received_clone.store(true, Ordering::Release);
        })
        .unwrap();
    }

    if opts.wait_seconds > 0.0 {
        let start = Instant::now();
        while start.elapsed().as_secs_f64() < opts.wait_seconds {
            std::thread::sleep(std::time::Duration::from_millis(20));
            if !opts.ignore_sigterm && signal_received.load(Ordering::Acquire) {
                break;
            }
        }
    }

    if opts.busy_wait_seconds > 0.0 {
        let start = Instant::now();
        while start.elapsed().as_secs_f64() < opts.busy_wait_seconds {
            // this is a busy wait
            if !opts.ignore_sigterm && signal_received.load(Ordering::Acquire) {
                break;
            }
        }
    }

    if let Some(msg) = opts.print.as_ref() {
        println!("{msg}");
    } else if let Ok(solution_path) =
        std::env::var("STRIDE_INSTANCE_PATH").map(|p| PathBuf::from(p).with_extension("out"))
    {
        println!(
            "#s s_demo_path \"{}\"",
            solution_path.as_os_str().to_str().unwrap_or_default()
        );

        let contents = std::fs::read_to_string(&solution_path)
            .unwrap_or_else(|_| panic!("Failed to read solution file {:?}", solution_path));
        print!("{}", contents);
    }

    if let Some(size) = opts.extra_alloc {
        let mut vec: Vec<u8> = black_box(vec![0u8; size]);
        vec.fill(1); // acutally access the memory
    }

    std::process::exit(opts.exit_code);
}
