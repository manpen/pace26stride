use crate::commands::arguments::CommandRunArgs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandRunError {}

pub fn command_run(_args: &CommandRunArgs) -> Result<(), CommandRunError> {
    Ok(())
}
