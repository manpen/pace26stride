use pace26checker::digest::digest_output::InstanceDigest;
use pace26remote::job_description::{JobDescription, JobResult};
use pace26remote::job_transfer::{TransferFromServer, TransferToServer};
use pace26remote::upload::UploadError;
use reqwest::{ClientBuilder, IntoUrl};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::timeout;
use tracing::{debug, error, trace};
use url::Url;

const UPLOAD_AGGREGATION_TIMEOUT: Duration = Duration::from_millis(500);
const UPLOAD_MAX_BUFFER_SIZE: usize = 200;

type ReturnChannel = oneshot::Sender<Option<u32>>;
type MessageToUploader = (Option<ReturnChannel>, JobDescription);

pub trait Uploader: Send + Sync {
    fn upload(
        &self,
        jobs: &[JobDescription],
    ) -> impl Future<Output = Result<HashMap<InstanceDigest, u32>, UploadError>> + Send;
}

pub struct UploadToStride {
    url: Url,
}

impl UploadToStride {
    pub fn new_with_server(into_url: impl IntoUrl) -> Result<UploadToStride, UploadError> {
        let url = into_url.into_url()?.join("/api/solution")?;
        Self::new_with_endpoint(url)
    }

    pub fn new_with_endpoint(into_url: impl IntoUrl) -> Result<Self, UploadError> {
        let url = into_url.into_url()?;
        Ok(UploadToStride { url })
    }
}

impl Uploader for UploadToStride {
    async fn upload(
        &self,
        jobs: &[JobDescription],
    ) -> Result<HashMap<InstanceDigest, u32>, UploadError> {
        let client = ClientBuilder::new()
            .danger_accept_invalid_certs(true)
            .build()?;

        let payload = TransferToServer {
            jobs: jobs.to_vec(),
        };
        let response = client.post(self.url.clone()).json(&payload).send().await?;
        trace!("Upload request received: {:?}", response);

        let deserialized: TransferFromServer = if !response.status().is_success() {
            error!(
                "Upload request returned an error: {} / {}",
                response.status(),
                response.text().await?
            );
            TransferFromServer {
                best_scores: Default::default(),
            }
        } else {
            response.json().await?
        };

        Ok(deserialized.best_scores)
    }
}

pub struct JobResultUploadAggregation {
    channel_to_upload: mpsc::UnboundedSender<MessageToUploader>,
    join_handle: JoinHandle<()>,
}

impl JobResultUploadAggregation {
    pub fn new<U: Uploader + 'static>(uploader: Arc<U>) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel::<MessageToUploader>();

        let join_handle = tokio::spawn(async move {
            let mut messages = Vec::new();
            let mut return_channels: HashMap<InstanceDigest, Vec<ReturnChannel>> = HashMap::new();
            let mut time_since_first = None;

            let mut keep_running = true;

            while keep_running {
                match timeout(UPLOAD_AGGREGATION_TIMEOUT, receiver.recv()).await {
                    Ok(Some((channel, msg))) => {
                        if let Some(channel) = channel {
                            return_channels
                                .entry(msg.idigest)
                                .or_default()
                                .push(channel);
                        }
                        messages.push(msg);
                        time_since_first = Some(time_since_first.unwrap_or_else(Instant::now));

                        if messages.len() < UPLOAD_MAX_BUFFER_SIZE
                            && time_since_first
                                .is_some_and(|i| i.elapsed() < UPLOAD_AGGREGATION_TIMEOUT)
                        {
                            continue;
                        }
                    }
                    Ok(None) => {
                        trace!("JobResultUpload senders dropped");
                        keep_running = false;
                    }
                    Err(_) => {
                        trace!("JobResultUpload task timed out");
                    }
                }

                if messages.is_empty() {
                    continue;
                }

                let best_known = uploader.upload(messages.as_slice()).await;
                messages.clear();
                time_since_first = None;
                trace!("Received best knowns from server: {:?}", best_known);

                match best_known {
                    Ok(best_known) => {
                        for (idigest, score) in best_known.into_iter() {
                            if let Some(channels) = return_channels.remove(&idigest) {
                                for channel in channels {
                                    let _ = channel.send(Some(score));
                                }
                            }
                        }
                    }
                    Err(err) => {
                        error!("Uploader failed: {err:?}");
                    }
                }

                for (_, channels) in return_channels.drain() {
                    for channel in channels {
                        let _ = channel.send(None);
                    }
                }
            }
        });

        Self {
            channel_to_upload: sender,
            join_handle,
        }
    }

    pub async fn upload_and_fetch_best_known(&self, desc: JobDescription) -> Option<u32> {
        let channel_to_upload = self.channel_to_upload.clone();

        if matches!(desc.result, JobResult::Valid { .. }) {
            // we only wait for an answer if the JobResult is valid
            let (sender, receiver) = oneshot::channel::<Option<u32>>();
            if let Err(e) = channel_to_upload.send((Some(sender), desc)) {
                debug!("Error sending job result upload: {e:?}");
                return None;
            }

            receiver.await.unwrap_or_else(|e| {
                debug!("Error receiving best known score: {e:?}");
                None
            })
        } else {
            if let Err(e) = channel_to_upload.send((None, desc)) {
                debug!("Error sending job result upload: {e:?}");
            }
            None
        }
    }

    pub async fn join(self) -> Result<(), JoinError> {
        self.join_handle.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn no_results_from_server() {
        let dummy_inst: InstanceDigest = "00000000000000000000000000000003".try_into().unwrap();

        let uploader = Arc::new(MockUploader::default());
        uploader.put(Ok(HashMap::new())).await;

        let aggr = Arc::new(JobResultUploadAggregation::new(uploader.clone()));

        let join0 = {
            let aggr = aggr.clone();
            tokio::spawn(async move {
                aggr.upload_and_fetch_best_known(JobDescription::valid(
                    dummy_inst,
                    Vec::new(),
                    None,
                ))
                .await
            })
        };

        let join1 = {
            let aggr = aggr.clone();
            tokio::spawn(async move {
                aggr.upload_and_fetch_best_known(JobDescription::infeasible(dummy_inst, None))
                    .await
            })
        };

        assert_eq!(
            timeout(5 * UPLOAD_AGGREGATION_TIMEOUT, join0)
                .await
                .unwrap()
                .unwrap(),
            None
        );
        assert_eq!(
            timeout(5 * UPLOAD_AGGREGATION_TIMEOUT, join1)
                .await
                .unwrap()
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn some_results_from_server() {
        let wo_response: InstanceDigest = "00000000000000000000000000000001".try_into().unwrap();
        let with_response: InstanceDigest = "00000000000000000000000000000003".try_into().unwrap();

        let uploader = Arc::new(MockUploader::default());
        uploader.put(Ok([(with_response, 12345)].into())).await;

        let aggr = Arc::new(JobResultUploadAggregation::new(uploader.clone()));

        let join_wo = {
            let aggr = aggr.clone();
            tokio::spawn(async move {
                aggr.upload_and_fetch_best_known(JobDescription::valid(
                    wo_response,
                    Vec::new(),
                    None,
                ))
                .await
            })
        };

        let join_with = {
            let aggr = aggr.clone();
            tokio::spawn(async move {
                aggr.upload_and_fetch_best_known(JobDescription::valid(
                    with_response,
                    Vec::new(),
                    None,
                ))
                .await
            })
        };

        assert_eq!(
            timeout(5 * UPLOAD_AGGREGATION_TIMEOUT, join_wo)
                .await
                .unwrap()
                .unwrap(),
            None
        );
        assert_eq!(
            timeout(5 * UPLOAD_AGGREGATION_TIMEOUT, join_with)
                .await
                .unwrap()
                .unwrap(),
            Some(12345)
        );
    }

    #[derive(Default)]
    struct MockUploader {
        response: Mutex<Option<Result<HashMap<InstanceDigest, u32>, UploadError>>>,
    }

    impl MockUploader {
        async fn put(&self, value: Result<HashMap<InstanceDigest, u32>, UploadError>) {
            let mut lock = self.response.lock().await;
            *lock = Some(value);
        }
    }

    impl Uploader for MockUploader {
        async fn upload(
            &self,
            _jobs: &[JobDescription],
        ) -> Result<HashMap<InstanceDigest, u32>, UploadError> {
            let mut lock = self.response.lock().await;
            lock.take().unwrap()
        }
    }
}
