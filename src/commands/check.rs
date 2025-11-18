use crate::commands::arguments::CommandCheckArgs;
use pace26checker::checks::checker::*;

pub type CommandCheckError = CheckerError;

pub fn command_check(args: &CommandCheckArgs) -> Result<(), CommandCheckError> {
    if !args.quiet {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_max_level(tracing::Level::INFO)
            .without_time()
            .init();
    }

    if let Some(solution_path) = args.solution.as_ref() {
        let size = check_instance_and_solution(&args.instance, solution_path, args.paranoid)?;
        println!("#s solution_size {size}");
    } else {
        check_instance_only(&args.instance, args.paranoid)?;
    }

    Ok(())
}
