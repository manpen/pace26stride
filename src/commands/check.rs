use crate::commands::arguments::CommandCheckArgs;
use pace26checker::{checks::checker::*, io::forest_dot_writer::ForestDotWriter};

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
        let (instance, solution, forests) = check_instance_and_solution(
            &args.instance,
            solution_path,
            args.paranoid,
            args.export_dot,
        )?;

        if let Some(instance) = instance
            && args.export_dot
        {
            let mut forest_writer = ForestDotWriter::new(&instance);
            forest_writer.color_leafs(&solution, &forests);

            let mut stdout = std::io::stdout().lock();
            forest_writer.write(&mut stdout).unwrap();
        }

        println!("#s solution_size {}", solution.num_trees());
    } else {
        let instance = check_instance_only(&args.instance, args.paranoid)?;

        if args.export_dot {
            let mut forest_writer = ForestDotWriter::new(&instance);
            let mut stdout = std::io::stdout().lock();
            forest_writer.write(&mut stdout).unwrap();
        }
    }

    Ok(())
}
