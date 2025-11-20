use pace26stride::commands::{
    arguments::{Arguments, parse_prog_arguments},
    check::{CommandCheckError, command_check},
    run::{CommandRunError, command_run},
};

use thiserror::Error;
use tracing::error;

#[derive(Debug, Error)]
enum MainError {
    #[error(transparent)]
    CommandCheckError(#[from] CommandCheckError),

    #[error(transparent)]
    CommandRunError(#[from] CommandRunError),
}

async fn dispatch_command(args: &Arguments) -> Result<(), MainError> {
    match args {
        Arguments::Check(args) => command_check(args)?,
        Arguments::Run(args) => command_run(args).await?,
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = parse_prog_arguments();

    let res = dispatch_command(&args).await;
    if let Err(e) = res {
        error!("{e}");
        std::process::exit(1)
    }
}
