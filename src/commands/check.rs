use crate::commands::arguments::CommandCheckArgs;
use pace26checker::digest::algo::{digest_instance, digest_solution};
use pace26checker::{checks::checker::*, io::forest_dot_writer::ForestDotWriter};
use pace26remote::job_description::JobDescription;
use pace26remote::upload::{Upload, UploadError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandCheckError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    Checker(#[from] CheckerError),
    #[error(transparent)]
    Upload(#[from] UploadError),
}

pub async fn command_check(args: &CommandCheckArgs) -> Result<(), CommandCheckError> {
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
            args.export_dot | args.hash | args.upload,
        )?;

        if let Some(instance) = &instance {
            if args.export_dot {
                let mut forest_writer = ForestDotWriter::new(instance);
                forest_writer.color_leafs(&solution, &forests);

                let mut stdout = std::io::stdout().lock();
                forest_writer.write(&mut stdout)?;
            }

            if args.hash | args.upload {
                let trees = instance
                    .trees()
                    .iter()
                    .map(|(_, t)| t.clone())
                    .collect::<Vec<_>>();
                let idigest = digest_instance(trees, instance.num_leaves);

                let trees = solution
                    .trees()
                    .iter()
                    .map(|(_, t)| t.clone())
                    .collect::<Vec<_>>();
                let score = trees.len();
                let sdigest = digest_solution(trees, score as u32);

                println!("#s idigest \"{idigest}\"");
                println!("#s sdigest \"{sdigest}\"");

                if args.upload {
                    let trees = solution
                        .trees()
                        .iter()
                        .map(|(_, t)| t.clone())
                        .collect::<Vec<_>>();
                    let desc = JobDescription::valid(idigest, trees, None);

                    let mut upload = Upload::new_with_server(args.solution_server.clone())?;
                    upload.add_job(desc);
                    upload.flush().await?;
                }
            }
        }

        println!("#s solution_size {}", solution.num_trees());
    } else {
        let instance = check_instance_only(&args.instance, args.paranoid)?;

        if args.export_dot {
            let mut forest_writer = ForestDotWriter::new(&instance);
            let mut stdout = std::io::stdout().lock();
            forest_writer.write(&mut stdout)?;
        }

        if args.hash {
            let trees = instance
                .trees()
                .iter()
                .map(|(_, t)| t.clone())
                .collect::<Vec<_>>();
            let idigest = digest_instance(trees, instance.num_leaves);

            println!("#s idigest \"{idigest}\"");
        }
    }

    Ok(())
}
