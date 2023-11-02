use anyhow::{anyhow, Context};
use async_recursion::async_recursion;
use async_trait::async_trait;
use reqwest::{Response, StatusCode, Url};
use semver::Version;
use slog_scope::debug;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[cfg(test)]
use mockall::automock;

use mithril_common::MITHRIL_API_VERSION_HEADER;

use crate::{MithrilError, MithrilResult};

/// Error tied with the Aggregator client
#[derive(Error, Debug)]
pub enum AggregatorClientError {
    /// Error raised when querying the aggregator returned a 5XX error.
    #[error("remote server technical error")]
    RemoteServerTechnical(#[source] MithrilError),

    /// Error raised when querying the aggregator returned a 4XX error.
    #[error("remote server logical error")]
    RemoteServerLogical(#[source] MithrilError),

    /// Error raised when the server API version mismatch the client API version.
    #[error("API version mismatch")]
    ApiVersionMismatch(#[source] MithrilError),

    /// HTTP subsystem error
    #[error("HTTP subsystem error")]
    SubsystemError(#[source] MithrilError),
}

/// What can be read from an [AggregatorClient].
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AggregatorRequest {
    /// Get a specific [certificate][crate::MithrilCertificate] from the aggregator
    GetCertificate {
        /// Hash of the certificate to retrieve
        hash: String,
    },
    /// Lists the aggregator [certificates][crate::MithrilCertificate]
    ListCertificates,
    /// Get a specific [mithril stake distribution][crate::MithrilStakeDistribution] from the aggregator
    GetMithrilStakeDistribution {
        /// Hash of the mithril stake distribution to retrieve
        hash: String,
    },
    /// Lists the aggregator [mithril stake distribution][crate::MithrilStakeDistribution]
    ListMithrilStakeDistributions,
    /// Get a specific [snapshot][crate::Snapshot] from the aggregator
    GetSnapshot {
        /// Digest of the snapshot to retrieve
        digest: String,
    },
    /// Lists the aggregator [snapshots][crate::Snapshot]
    ListSnapshots,
}

impl AggregatorRequest {
    /// Get the request route relative to the aggregator root url.
    pub fn route(&self) -> String {
        match self {
            AggregatorRequest::GetCertificate { hash } => {
                format!("certificate/{hash}")
            }
            AggregatorRequest::ListCertificates => "certificates".to_string(),
            AggregatorRequest::GetMithrilStakeDistribution { hash } => {
                format!("artifact/mithril-stake-distribution/{hash}")
            }
            AggregatorRequest::ListMithrilStakeDistributions => {
                "artifact/mithril-stake-distributions".to_string()
            }
            AggregatorRequest::GetSnapshot { digest } => {
                format!("artifact/snapshot/{}", digest)
            }
            AggregatorRequest::ListSnapshots => "artifact/snapshots".to_string(),
        }
    }
}

/// API that defines a client for the Aggregator
#[async_trait]
pub trait AggregatorClient: Sync + Send {
    /// Get the content back from the Aggregator
    async fn get_content(
        &self,
        request: AggregatorRequest,
    ) -> Result<String, AggregatorClientError>;
}

/// Responsible of HTTP transport and API version check.
pub struct AggregatorHTTPClient {
    http_client: reqwest::Client,
    aggregator_url: Url,
    api_versions: Arc<RwLock<Vec<Version>>>,
}

impl AggregatorHTTPClient {
    /// AggregatorHTTPClient factory
    pub fn new(aggregator_endpoint: Url, api_versions: Vec<Version>) -> MithrilResult<Self> {
        debug!("New AggregatorHTTPClient created");
        let http_client = reqwest::ClientBuilder::new()
            .build()
            .with_context(|| "Building http client for Aggregator client failed")?;

        Ok(Self {
            http_client,
            aggregator_url: aggregator_endpoint,
            api_versions: Arc::new(RwLock::new(api_versions)),
        })
    }

    /// Computes the current api version
    async fn compute_current_api_version(&self) -> Option<Version> {
        self.api_versions.read().await.first().cloned()
    }

    /// Discards the current api version
    /// It discards the current version if and only if there is at least 2 versions available
    async fn discard_current_api_version(&self) -> Option<Version> {
        if self.api_versions.read().await.len() < 2 {
            return None;
        }
        if let Some(current_api_version) = self.compute_current_api_version().await {
            let mut api_versions = self.api_versions.write().await;
            if let Some(index) = api_versions
                .iter()
                .position(|value| *value == current_api_version)
            {
                api_versions.remove(index);
                return Some(current_api_version);
            }
        }
        None
    }

    /// Perform a HTTP GET request on the Aggregator and return the given JSON
    #[async_recursion]
    async fn get(&self, url: Url) -> Result<Response, AggregatorClientError> {
        debug!("GET url='{url}'.");
        let request_builder = self.http_client.get(url.clone());
        let current_api_version = self
            .compute_current_api_version()
            .await
            .unwrap()
            .to_string();
        debug!("Prepare request with version: {current_api_version}");
        let request_builder =
            request_builder.header(MITHRIL_API_VERSION_HEADER, current_api_version);
        let response = request_builder.send().await.map_err(|e| {
            AggregatorClientError::SubsystemError(anyhow!(e).context(format!(
                "Cannot perform a GET against the Aggregator HTTP server (url='{url}')"
            )))
        })?;

        match response.status() {
            StatusCode::OK => Ok(response),
            StatusCode::PRECONDITION_FAILED => {
                if self.discard_current_api_version().await.is_some()
                    && !self.api_versions.read().await.is_empty()
                {
                    return self.get(url).await;
                }

                Err(self.handle_api_error(&response).await)
            }
            StatusCode::NOT_FOUND => Err(AggregatorClientError::RemoteServerLogical(anyhow!(
                "Url='{url} not found"
            ))),
            status_code => Err(AggregatorClientError::RemoteServerTechnical(anyhow!(
                "Unhandled error {status_code}"
            ))),
        }
    }

    /// API version error handling
    async fn handle_api_error(&self, response: &Response) -> AggregatorClientError {
        if let Some(version) = response.headers().get(MITHRIL_API_VERSION_HEADER) {
            AggregatorClientError::ApiVersionMismatch(anyhow!(
                "server version: '{}', signer version: '{}'",
                version.to_str().unwrap(),
                self.compute_current_api_version().await.unwrap()
            ))
        } else {
            AggregatorClientError::ApiVersionMismatch(anyhow!(
                "version precondition failed, sent version '{}'.",
                self.compute_current_api_version().await.unwrap()
            ))
        }
    }

    fn get_url_for_route(&self, endpoint: &str) -> Result<Url, AggregatorClientError> {
        self.aggregator_url
            .join(endpoint)
            .with_context(|| {
                format!(
                    "Invalid url when joining given endpoint, '{endpoint}', to aggregator url '{}'",
                    self.aggregator_url
                )
            })
            .map_err(AggregatorClientError::SubsystemError)
    }
}

#[cfg_attr(test, automock)]
#[async_trait]
impl AggregatorClient for AggregatorHTTPClient {
    async fn get_content(
        &self,
        request: AggregatorRequest,
    ) -> Result<String, AggregatorClientError> {
        let response = self.get(self.get_url_for_route(&request.route())?).await?;
        let content = format!("{response:?}");

        response.text().await.map_err(|e| {
            AggregatorClientError::SubsystemError(anyhow!(e).context(format!(
                "Could not find a JSON body in the response '{content}'."
            )))
        })
    }
}
