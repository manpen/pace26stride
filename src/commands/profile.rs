use std::mem::MaybeUninit;
use std::process::{Stdio, exit};
use std::time::{Instant};

use super::arguments::CommandProfileArgs;
use crate::job::job_processor::OOM_EXIT_CODE;
use libc::{rlimit, rusage, setrlimit};
use thiserror::Error;
use tokio::process::Command;
use tokio::signal::unix::{SignalKind, signal};

#[derive(Debug, Error)]
pub enum CommandProfileError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// the actual return type should be Result<!, ..> --- since we only return in case of error,
// but the `!` type seems to be still experimental
pub async fn command_profile(args: &CommandProfileArgs) -> Result<(), CommandProfileError> {
    // we are using the blocking variant here, since we have nothing else to do anyhow
    let start = Instant::now();

    let mut command = Command::new(args.solver.clone());
    let command = command
        .args(args.solver_args.clone())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if let Some(memory_limit_in_mib) = args.memory_limit {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        unsafe {
            command.pre_exec(move || {
                let limit: u64 = memory_limit_in_mib as u64 * 1024 * 1024; // 512 MB

                let rlim = rlimit {
                    rlim_cur: limit,
                    rlim_max: limit,
                };

                #[cfg(target_os = "linux")]
                if setrlimit(libc::RLIMIT_AS, &rlim) != 0 {
                    return Err(std::io::Error::last_os_error());
                }

                #[cfg(target_os = "macos")]
                if setrlimit(libc::RLIMIT_RSS, &rlim) != 0 {
                    return Err(std::io::Error::last_os_error());
                }

                Ok(())
            });
        }
    }

    let mut child = command.spawn()?;

    let mut stream_sigint = signal(SignalKind::interrupt())?;
    let mut stream_sigterm = signal(SignalKind::terminate())?;

    let code = loop {
        tokio::select! {
            _ = stream_sigint.recv() => {
                child.kill().await?;
            },

            _ = stream_sigterm.recv() => {
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                }
            },

            status = child.wait() => {
                break status?.code();
            }
        }
    };

    println!("#s s_wtime {}", start.elapsed().as_secs_f64());

    assert!(
        child.id().is_none(),
        "This point should only be reached if the child has terminated"
    );

    let usage = get_rusage_children();
    report_usage(usage);

    if let Some(memory_limit_in_mib) = args.memory_limit
        && code.is_none()
        && usage.ru_maxrss as usize > memory_limit_in_mib * 1024 * 1024
    {
        println!("#s out_of_memory true");
        exit(OOM_EXIT_CODE);
    }

    exit(code.unwrap_or(1));
}

fn get_rusage_children() -> rusage {
    use libc::*;

    let mut usage = MaybeUninit::<rusage>::uninit();

    unsafe {
        let ret = getrusage(RUSAGE_CHILDREN, usage.as_mut_ptr());
        if ret != 0 {
            panic!("getrusage() failed");
        }
        usage.assume_init()
    }
}

fn report_usage(usage: rusage) {
    let utime = usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1_000_000.0;
    let stime = usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1_000_000.0;

    #[cfg(target_os = "linux")]
    let maxrss = usage.ru_maxrss * 1024;
    #[cfg(target_os = "macos")]
    let maxrss = usage.ru_maxrss;

    println!("#s s_utime {utime}");
    println!("#s s_stime {stime}");
    println!("#s s_maxrss {maxrss}");
    println!("#s s_minflt {}", usage.ru_minflt);
    println!("#s s_majflt {}", usage.ru_majflt);
    println!("#s s_nvcsw {}", usage.ru_nvcsw);
    println!("#s s_nivcsw {}", usage.ru_nivcsw);
}
