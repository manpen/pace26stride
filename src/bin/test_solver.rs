use clap::Parser;
/// This binary is used to test the runner.
///  Standard behavior is to print out the contents of the file "{X}.out" where X is the value of the environment variable "STRIDE_INSTANCE_PATH".
///  It also supports additional options to wait, ignore SIGTERM, and set exit code.
use std::{hint::black_box, path::PathBuf};

#[derive(Parser)]
struct Opts {
    #[arg(
        short,
        long,
        help = "Number of seconds to wait before output",
        default_value = "0"
    )]
    wait_seconds: f64,

    #[arg(short = 'm', long, help = "Allocate extra memory (in bytes)")]
    extra_alloc: Option<usize>,

    #[arg(short, long, help = "Ignore SIGTERM signal")]
    ignore_sigterm: bool,

    #[arg(short = 'e', long, help = "Exit code to return", default_value = "0")]
    exit_code: i32,
}

fn main() {
    let opts = Opts::parse();

    if opts.ignore_sigterm {
        let _ = ctrlc::set_handler(|| {
            // Ignore SIGTERM
        });
    }

    if opts.wait_seconds > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(opts.wait_seconds));
    }

    if let Ok(solution_path) =
        std::env::var("STRIDE_INSTANCE_PATH").map(|p| PathBuf::from(p).with_extension("out"))
    {
        println!(
            "#s path \"{}\"",
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
