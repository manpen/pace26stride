use pace26stride::commands::{
    arguments::{Arguments, parse_prog_arguments},
    check::{CommandCheckError, command_check},
    download::command_download,
    profile::{CommandProfileError, command_profile},
    run::{CommandRunError, command_run},
};

use pace26stride::commands::download::CommandDownloadError;
use thiserror::Error;

#[derive(Debug, Error)]
enum MainError {
    #[error(transparent)]
    Check(#[from] CommandCheckError),

    #[error(transparent)]
    Run(#[from] CommandRunError),

    #[error(transparent)]
    Profile(#[from] CommandProfileError),

    #[error(transparent)]
    Download(#[from] CommandDownloadError),
}

async fn dispatch_command(args: &Arguments) -> Result<(), MainError> {
    match args {
        Arguments::Check(args) => command_check(args).await?,
        Arguments::Run(args) => command_run(args).await?,
        Arguments::Profile(args) => command_profile(args).await?,
        Arguments::Download(args) => command_download(args).await?,
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let args = parse_prog_arguments();

    let res = dispatch_command(&args).await;
    if let Err(e) = res {
        println!("{}: {e}", console::Style::new().red().apply_to("Error:"));
        println!("Debug: {e:?}");
        std::process::exit(1)
    }
}
