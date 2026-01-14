use crate::commands::arguments::CommandDownloadArgs;
use crate::instances::directory::InstanceDirectory;
use crate::instances::parser::InstanceSource::StrideInstance;
use crate::instances::parser::{InstanceSourceParser, collect_instances_from_args};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use pace26checker::digest::digest_output::InstanceDigest;
use rand::prelude::SliceRandom;
use reqwest::{Client, ClientBuilder};
use thiserror::Error;
use tokio::fs::File;
use tracing::info;

use async_compression::tokio::bufread::GzipDecoder;
use console::Style;
use indicatif::{ProgressBar, ProgressStyle};
use std::io;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::sync::Semaphore;
use tokio_util::io::StreamReader;
use url::Url;

const DOWNLOAD_CHUNK_SIZE: usize = 50; // keep request url below 2kb
const CONCURRENT_DOWNLOADS: usize = 2;

#[derive(Error, Debug)]
pub enum CommandDownloadError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    InstanceSource(#[from] InstanceSourceParser),
    #[error(transparent)]
    UrlParse(#[from] url::ParseError),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    JoinError(#[from] tokio::task::JoinError),
}

pub async fn command_download(args: &CommandDownloadArgs) -> Result<(), CommandDownloadError> {
    if !args.quiet {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_max_level(tracing::Level::INFO)
            .without_time()
            .init();
    }

    let download_path = InstanceDirectory::new(args.downloads_path.clone());
    let (num_orig_requested, missing_digests) = collect_missing_digests(args, &download_path)?;
    if missing_digests.is_empty() {
        return Ok(());
    }

    println!(
        "Store downloads into {}",
        Style::new()
            .blue()
            .apply_to(download_path.path().display().to_string())
    );

    let file_progress = ProgressBar::new(missing_digests.len() as u64);
    file_progress.set_style(ProgressStyle::with_template("{msg:<15.cyan} [{elapsed_precise:.cyan}] [{wide_bar:.cyan/grey}] {human_pos.cyan} of {human_len} (est: {eta})").unwrap()
                                .progress_chars("#>-"));
    file_progress.set_message("Downloading...");
    file_progress.inc((num_orig_requested - missing_digests.len()) as u64);

    let client = ClientBuilder::new().build()?;
    let server_url = Arc::new(args.stride_server.clone());
    let download_path = Arc::new(download_path);

    let mut digests_left = missing_digests.as_slice();
    let semaphore = Arc::new(Semaphore::new(CONCURRENT_DOWNLOADS));

    let start = Instant::now();

    let mut join_handlers = Vec::with_capacity(2 * CONCURRENT_DOWNLOADS);
    while !digests_left.is_empty() {
        let current_digests = digests_left
            .split_off(..DOWNLOAD_CHUNK_SIZE.min(digests_left.len()))
            .unwrap()
            .into();

        let permit = semaphore.clone().acquire_owned().await.unwrap();

        let client = client.clone();
        let server_url = server_url.clone();
        let download_path = download_path.clone();
        let file_progress_bar = file_progress.clone();

        join_handlers.push(tokio::spawn(async move {
            let res = download_chunk(
                client,
                server_url,
                download_path,
                current_digests,
                file_progress_bar,
            )
            .await;
            drop(permit);
            res
        }));

        join_handlers.retain(|h| !h.is_finished());
    }

    for handler in join_handlers {
        handler.await??;
        file_progress.tick();
    }

    file_progress.finish();

    println!("Finished in {}ms", start.elapsed().as_millis());

    Ok(())
}

async fn download_chunk(
    client: Client,
    server_url: Arc<Url>,
    download_path: Arc<InstanceDirectory>,
    digests: Vec<InstanceDigest>,
    file_progress_bar: ProgressBar,
) -> Result<(), CommandDownloadError> {
    let reqwest_stream = request_digest_stream(&client, &server_url, &digests).await?;

    let mut lines = decompress_stream(reqwest_stream);

    let mut writer = None;
    while let Some(line) = lines.next_line().await? {
        // A stride idigest line indicates the start of an instance, thus we open a new file
        if let Some(digest) = parse_stride_idigest_line(&line)? {
            let new_writer = open_instance_writer(&download_path, &digest).await?;
            if let Some(mut old) = writer.replace(new_writer) {
                old.flush().await?;
                file_progress_bar.inc(1);
            }
        }

        // Move line into writer
        if let Some(writer) = &mut writer {
            writer.write_all(line.as_bytes()).await?;
            writer.write_all("\n".as_bytes()).await?;
        } else {
            info!("Received content without expected header; ignoring: {line}");
        }
    }

    if let Some(mut writer) = writer {
        writer.flush().await?;
        file_progress_bar.inc(1);
    }
    Ok(())
}

async fn open_instance_writer(
    download_path: &InstanceDirectory,
    digest: &InstanceDigest,
) -> Result<BufWriter<File>, CommandDownloadError> {
    let path = download_path.path_of_digest(digest);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let file = File::create(&path).await?;

    let new_writer = BufWriter::new(file);

    Ok(new_writer)
}

fn parse_stride_idigest_line(line: &str) -> Result<Option<InstanceDigest>, serde_json::Error> {
    const IDIGEST_PREFIX: &str = "#s idigest ";

    if let Some(digest_str) = line.strip_prefix(IDIGEST_PREFIX) {
        Ok(Some(serde_json::from_str(digest_str)?))
    } else {
        Ok(None)
    }
}

fn decompress_stream(
    reqwest_stream: impl Stream<Item = reqwest::Result<Bytes>>,
) -> Lines<impl AsyncBufRead> {
    // need to err_map to be compatible with StreamReader
    let stream = reqwest_stream.map(|r| r.map_err(io::Error::other));

    let reader = StreamReader::new(stream);
    let buf_reader = BufReader::new(reader);

    // now use multi member decoder (this allows the server to concat gzips)
    let mut decoder = GzipDecoder::new(buf_reader);
    decoder.multiple_members(true);

    // assume that each file starts with the idigest line
    let buffered = BufReader::new(decoder);
    buffered.lines()
}

async fn request_digest_stream(
    client: &Client,
    server_url: &Url,
    current_digests: &[InstanceDigest],
) -> Result<impl Stream<Item = reqwest::Result<Bytes>>, CommandDownloadError> {
    let request_url = {
        let mut digests_param = String::with_capacity(current_digests.len() * 33 + 5);
        digests_param.push_str("i=");
        for (i, d) in current_digests.iter().enumerate() {
            if i != 0 {
                digests_param.push(',');
            }
            digests_param.push_str(d.to_string().as_str());
        }
        let mut url = server_url.join("api/instances")?;
        url.set_query(Some(&digests_param));
        url
    };

    let response = client.get(request_url).send().await?.error_for_status()?; // fail on non-2xx
    Ok(response.bytes_stream())
}

fn collect_missing_digests(
    args: &CommandDownloadArgs,
    download_path: &InstanceDirectory,
) -> Result<(usize, Vec<InstanceDigest>), CommandDownloadError> {
    let mut requested_stride_instances: Vec<_> = collect_instances_from_args(&args.instances)?
        .into_iter()
        .filter_map(|instance| {
            if let StrideInstance(digest) = instance.instance_source {
                Some(digest)
            } else {
                println!(
                    "Ignore non-stride instance {}. Did you forget the {} prefix?",
                    Style::new().red().apply_to(format!("{instance:?}")),
                    Style::new().yellow().apply_to("s:")
                );
                None
            }
        })
        .collect();

    requested_stride_instances.sort_unstable_by(|a, b| b.cmp(a)); // Reversed
    requested_stride_instances.dedup();

    if requested_stride_instances.is_empty() {
        println!(
            "No stride instances requested. Did you forget the {} prefix?",
            Style::new().yellow().apply_to("s:")
        );
    }

    let num_requested = requested_stride_instances.len();

    // map stride ids to path in download folder
    let mut non_existing_digests: Vec<_> = requested_stride_instances
        .into_iter()
        .filter(|d| args.replace_existing || !download_path.path_of_digest(d).exists())
        .collect();

    if non_existing_digests.is_empty() {
        println!(
            "No stride instances missing. If you want to fetch existing instances again use the argument {}",
            Style::new().yellow().apply_to("--replace-existing")
        );
    }

    non_existing_digests.shuffle(&mut rand::rng());

    Ok((num_requested, non_existing_digests))
}
