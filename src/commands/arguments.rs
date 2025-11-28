use std::{num::ParseFloatError, path::PathBuf, time::Duration};

use structopt::StructOpt;

pub const ENV_SOLVER: &str = "STRIDE_SOLVER";
pub const ENV_SOFT_TIMEOUT: &str = "STRIDE_TIMEOUT";
pub const ENV_GRACE_PERIOD: &str = "STRIDE_GRACE";
pub const ENV_PARALLEL_JOBS: &str = "STRIDE_PARALLEL";
pub const ENV_REQUIRE_OPTIMAL: &str = "STRIDE_OPTIMAL";
pub const ENV_KEEP_LOGS: &str = "STRIDE_KEEP";

#[derive(StructOpt, Debug)]
pub enum Arguments {
    #[structopt(about = "Check a solution file")]
    Check(CommandCheckArgs),

    #[structopt(about = "Run solver and postprocess solution")]
    Run(CommandRunArgs),
}

#[derive(StructOpt, Debug, Default)]
pub struct CommandCheckArgs {
    #[structopt(help = "Path to instance file")]
    pub instance: PathBuf,

    #[structopt(help = "Path to solution file; if omitted, only instance is checked")]
    pub solution: Option<PathBuf>,

    #[structopt(short, long, help = "Produce as little output as possible")]
    pub quiet: bool,

    #[structopt(short, long, help = "Stricter linting and all warnings become errors")]
    pub paranoid: bool,

    #[structopt(short = "d", help = "If input is valid, export it as GraphViz dot")]
    pub export_dot: bool,
}

#[derive(StructOpt, Debug, Default, Clone)]
pub struct CommandRunArgs {
    #[structopt(short, long, help = "List of instance files", required = true)]
    pub instances: Vec<PathBuf>,

    #[structopt(short, long, env = ENV_SOLVER, help = "Solver program to execute")]
    pub solver: PathBuf,

    #[structopt(short="t", long="timeout", env = ENV_SOFT_TIMEOUT, parse(try_from_str = parse_duration), help = "Solver time budget in seconds (then SIGTERM)", default_value="30")]
    pub soft_timeout: Duration,

    #[structopt(short="g", long="grace", env = ENV_GRACE_PERIOD, parse(try_from_str = parse_duration), help = "Seconds between SIGTERM and SIGKILL", default_value="5")]
    pub grace_period: Duration,

    #[structopt(
        short = "p",
        long = "parallel",
        env = ENV_PARALLEL_JOBS,
        help = "Number of solvers to run in parallel; default: number of physical cores"
    )]
    pub parallel_jobs: Option<u64>,

    #[structopt(
        short = "o",
        long = "optimal",
        env = ENV_REQUIRE_OPTIMAL,
        help = "Treat suboptimal solutions as error"
    )]
    pub require_optimal: bool,

    #[structopt(
        short = "k",
        long = "keep-logs",
        env = ENV_KEEP_LOGS,
        help = "Keep logs of successful runs"
    )]
    pub keep_successful_logs: bool,

    #[structopt(last = true, help = "Arguments passed to solver")]
    pub solver_args: Vec<String>,
}

fn parse_duration(src: &str) -> Result<Duration, ParseFloatError> {
    let seconds: f64 = src.parse()?;
    Ok(Duration::from_secs_f64(seconds))
}

fn default_parallel_jobs() -> u64 {
    num_cpus::get_physical() as u64
}

pub fn parse_prog_arguments() -> Arguments {
    let mut opts = Arguments::from_args();

    if let Arguments::Run(opts) = &mut opts {
        if opts.parallel_jobs.is_none() {
            opts.parallel_jobs = Some(default_parallel_jobs());
        }

        if opts.instances.is_empty() {
            panic!("No instance provided using --instance argument");
        }

        if opts.solver.parent().is_none() && !opts.solver.starts_with("./") {
            // TODO: We could automatically fix this instead of panicking.
            // But it seems to be better to make the user aware of this.
            panic!(
                "It seems like you provided a relative solver path without './' prefix. Please add './' to the solver path or provide an absolute path."
            );
        }
    }

    opts
}
