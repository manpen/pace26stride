use clap::Parser;
use std::{path::PathBuf, time::Duration};
use tracing::error;
use url::Url;

pub const ENV_SOLVER: &str = "STRIDE_SOLVER";
pub const ENV_SOFT_TIMEOUT: &str = "STRIDE_TIMEOUT";
pub const ENV_GRACE_PERIOD: &str = "STRIDE_GRACE";
pub const ENV_PARALLEL_JOBS: &str = "STRIDE_PARALLEL";
pub const ENV_REQUIRE_OPTIMAL: &str = "STRIDE_OPTIMAL";
pub const ENV_KEEP_LOGS: &str = "STRIDE_KEEP";
pub const ENV_STRIDE_MAX_RUN_LOGS: &str = "STRIDE_MAX_RUN_LOGS";
pub const ENV_STRIDE_SERVER: &str = "STRIDE_SERVER";
pub const STRIDE_SERVER_DEFAULT: &str = "https://pace2026.imada.sdu.dk/";

#[derive(Parser, Debug)]
pub enum Arguments {
    #[command(alias = "c", visible_alias = "verify", about = "Check a solution file")]
    Check(CommandCheckArgs),

    #[command(alias = "r", about = "Run solver and postprocess solution")]
    Run(CommandRunArgs),

    #[command(alias = "p", hide = true)]
    Profile(CommandProfileArgs),
}

#[derive(Parser, Debug, Default)]
pub struct CommandProfileArgs {
    #[arg(help = "Solver program to execute")]
    pub solver: PathBuf,

    #[arg(help = "Arguments passed to solver")]
    pub solver_args: Vec<String>,
}

#[derive(Parser, Debug)]
pub struct CommandCheckArgs {
    #[arg(help = "Path to instance file")]
    pub instance: PathBuf,

    #[arg(help = "Path to solution file; if omitted, only instance is checked")]
    pub solution: Option<PathBuf>,

    #[arg(short, long, help = "Produce as little output as possible")]
    pub quiet: bool,

    #[arg(short, long, help = "Stricter linting and all warnings become errors")]
    pub paranoid: bool,

    #[arg(
        short = 'd',
        long,
        help = "If input is valid, export it as GraphViz dot"
    )]
    pub export_dot: bool,

    #[arg(short = 'H', long, help = "Compute hash of instance [and solution]")]
    pub hash: bool,

    #[arg(short = 'S', long, env = ENV_STRIDE_SERVER, default_value = STRIDE_SERVER_DEFAULT, help = "Server to upload to")]
    pub solution_server: Url,

    #[arg(short = 'u', long, help = "Upload solution of stride instances")]
    pub upload: bool,
}

#[derive(Parser, Debug, Clone)]
pub struct CommandRunArgs {
    #[arg(short, long, env = ENV_SOLVER, help = "Solver program to execute")]
    pub solver: PathBuf,

    #[arg(short, long, help = "List of instance files", required = true, num_args(1..))]
    pub instances: Vec<PathBuf>,

    #[arg(short='t', long="timeout", env = ENV_SOFT_TIMEOUT, value_parser = parse_duration, help = "Solver time budget in seconds (then SIGTERM)", default_value="30")]
    pub soft_timeout: Duration,

    #[arg(short='g', long="grace", env = ENV_GRACE_PERIOD, value_parser = parse_duration, help = "Seconds between SIGTERM and SIGKILL", default_value="5")]
    pub grace_period: Duration,

    #[arg(
        short = 'p',
        long = "parallel",
        env = ENV_PARALLEL_JOBS,
        help = "Number of solvers to run in parallel; default: number of physical cores"
    )]
    pub parallel_jobs: Option<u64>,

    #[arg(
        short = 'o',
        long = "optimal",
        env = ENV_REQUIRE_OPTIMAL,
        help = "Treat suboptimal solutions as error, e.g. keep logs of suboptimal runs"
    )]
    pub require_optimal: bool,

    #[arg(
        short = 'k',
        long = "keep-logs",
        env = ENV_KEEP_LOGS,
        help = "Keep logs of successful runs"
    )]
    pub keep_successful_logs: bool,

    #[arg(
        short = 'P',
        long,
        help = "Do not record performance metrics; may increase performance"
    )]
    pub no_profile: bool,

    #[arg(
        short = 'E',
        long,
        help = "Do not set STRIDE_* enviroment variable for solver"
    )]
    pub no_envs: bool,

    #[arg(last = true, help = "Arguments passed to solver")]
    pub solver_args: Vec<String>,

    #[arg(short = 'S', long, env = ENV_STRIDE_SERVER, default_value = STRIDE_SERVER_DEFAULT, help = "Server to upload to")]
    pub solution_server: Url,

    #[arg(short = 'O', long, help = "Do not communicate with STRIDE servers")]
    pub offline: bool,

    #[arg(short = 'r', long="max_run_logs", env = ENV_STRIDE_MAX_RUN_LOGS, help="If more run logs are in the stride-log dir, remove oldest ones")]
    pub remove_old_logs: Option<usize>,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    s.parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|e| format!("Invalid duration: {}", e))
}
fn default_parallel_jobs() -> u64 {
    num_cpus::get_physical() as u64
}

pub fn parse_prog_arguments() -> Arguments {
    let mut opts = Arguments::parse();

    if let Arguments::Run(opts) = &mut opts {
        if opts.parallel_jobs.is_none() {
            opts.parallel_jobs = Some(default_parallel_jobs());
        }

        if opts.instances.is_empty() {
            panic!("No instance provided using --instance argument");
        }

        if opts.solver.parent().is_none_or(|x| x == "") && !opts.solver.starts_with("./") {
            // TODO: We could automatically fix this instead of panicking.
            // But it seems to be better to make the user aware of this.
            error!("Relative solver path without ./");
            panic!(
                "It seems like you provided a relative solver path without './' prefix. Please add './' to the solver path or provide an absolute path."
            );
        }
    }

    opts
}
