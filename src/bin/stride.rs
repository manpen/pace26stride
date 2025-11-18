use pace26stride::commands::{
    arguments::{Arguments, parse_prog_arguments},
    check::{CommandCheckError, command_check},
};

use thiserror::Error;
use tracing::error;

#[derive(Debug, Error)]
enum MainError {
    #[error(transparent)]
    CommandCheckError(#[from] CommandCheckError),
}

fn dispatch_command(args: Arguments) -> Result<(), MainError> {
    match args {
        Arguments::Check(args) => command_check(args)?,
        Arguments::Run(_args) => todo!(),
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = parse_prog_arguments();

    let res = dispatch_command(args);
    if let Err(e) = res {
        error!("{e}");
        std::process::exit(1)
    }
}
