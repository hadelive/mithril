use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use flate2::read::GzDecoder;
use mithril_common::{
    certificate_chain::CertificateVerifier,
    crypto_helper::{key_decode_hex, ProtocolGenesisVerifier},
    digesters::{CardanoImmutableDigester, ImmutableDigester},
    entities::{ProtocolMessagePartKey, Snapshot},
    StdError, StdResult,
};
use serde::{Deserialize, Serialize};
use tar::Archive;
use thiserror::Error;

use crate::aggregator_client::{AggregatorHTTPClientError, CertificateClient, SnapshotClient};

/// [AggregatorHandler] related errors.
#[derive(Error, Debug)]
pub enum SnapshotServiceError {
    /// The given identifier does not link to an existing snapshot.
    #[error("Snapshot '{0}' not found")]
    SnapshotNotFound(String),

    /// Error raised when the certificate verification failed for the downloaded archive.
    #[error("Certificate verification failed for snapshot '{digest}'. The archive has been downloaded as '{path}'.")]
    CouldNotVerifySnapshot {
        /// The identifier of the snapshot
        digest: String,
        /// The path of the downloaded archive
        path: PathBuf,
    },

    /// The given certificate could not be found, contains the certificate hash
    #[error("Could not find certificate '{0}'.")]
    CouldNotFindCertificate(String),
}

/// ## SnapshotService
///
/// This trait is the interface for the Snapshot service used in the main commands.
#[async_trait]
pub trait SnapshotService: Sync + Send {
    /// Return the list of the snapshots stored by the Aggregator.
    async fn list(&self) -> StdResult<Vec<Snapshot>>;

    /// Show details of the snapshot identified by the given digest.
    async fn show(&self, digest: &str) -> StdResult<Snapshot>;

    /// Download and verify the snapshot identified by the given digest.
    async fn download(
        &self,
        digest: &str,
        pathdir: &Path,
        genesis_verification_key: &str,
    ) -> StdResult<PathBuf>;
}

#[derive(Debug, Serialize, Deserialize)]
/// Configuration related to the [SnapshotService].
pub struct SnapshotConfig {
    /// Aggregator URL
    pub aggregator_endpoint: String,

    /// Genesis verification key
    pub genesis_verification_key: String,
}

/// Service used by the Command to perform business oriented tasks.
pub struct MithrilClientSnapshotService {
    /// Snapshot HTTP client
    snapshot_client: Arc<SnapshotClient>,

    /// Certificate HTTP client
    certificate_client: Arc<CertificateClient>,

    /// Certificate verifier
    certificate_verifier: Arc<dyn CertificateVerifier>,
}

impl MithrilClientSnapshotService {
    /// Create a new instance of the service.
    pub fn new(
        snapshot_client: Arc<SnapshotClient>,
        certificate_client: Arc<CertificateClient>,
        certificate_verifier: Arc<dyn CertificateVerifier>,
    ) -> Self {
        Self {
            snapshot_client,
            certificate_client,
            certificate_verifier,
        }
    }

    async fn unpack_snapshot(&self, filepath: &Path) -> StdResult<PathBuf> {
        let snapshot_file_tar_gz = File::open(filepath)?;
        let snapshot_file_tar = GzDecoder::new(snapshot_file_tar_gz);
        let unpack_dir_path = filepath.parent().unwrap().join(Path::new("db"));
        let mut snapshot_archive = Archive::new(snapshot_file_tar);
        snapshot_archive.unpack(&unpack_dir_path)?;

        Ok(unpack_dir_path)
    }
}

#[async_trait]
impl SnapshotService for MithrilClientSnapshotService {
    async fn list(&self) -> StdResult<Vec<Snapshot>> {
        self.snapshot_client.list().await
    }

    async fn show(&self, digest: &str) -> StdResult<Snapshot> {
        let snapshot =
            self.snapshot_client
                .show(digest)
                .await
                .map_err(|e| match &e.downcast_ref::<AggregatorHTTPClientError>() {
                    Some(error)
                        if matches!(error, &&AggregatorHTTPClientError::RemoteServerLogical(_)) =>
                    {
                        Box::new(SnapshotServiceError::SnapshotNotFound(digest.to_owned()))
                            as StdError
                    }
                    _ => e,
                })?;

        Ok(snapshot)
    }

    async fn download(
        &self,
        digest: &str,
        pathdir: &Path,
        genesis_verification_key: &str,
    ) -> StdResult<PathBuf> {
        let genesis_verification_key = key_decode_hex(&genesis_verification_key.to_string())?;
        let snapshot = self.snapshot_client.show(digest).await?;
        let filepath = self.snapshot_client.download(&snapshot, pathdir).await?;
        let genesis_verifier =
            ProtocolGenesisVerifier::from_verification_key(genesis_verification_key);
        let unpacked_path = self.unpack_snapshot(&filepath).await?;
        let digester = Box::new(CardanoImmutableDigester::new(
            Path::new(&unpacked_path).into(),
            None,
            slog_scope::logger(),
        ));
        let certificate = self
            .certificate_client
            .get(&snapshot.certificate_hash)
            .await?
            .ok_or_else(|| {
                SnapshotServiceError::CouldNotFindCertificate(snapshot.certificate_hash.clone())
            })?;
        let unpacked_snapshot_digest = digester.compute_digest(&certificate.beacon).await?;
        let mut protocol_message = certificate.protocol_message.clone();
        protocol_message.set_message_part(
            ProtocolMessagePartKey::SnapshotDigest,
            unpacked_snapshot_digest.clone(),
        );
        if protocol_message.compute_hash() != certificate.signed_message {
            return Err(SnapshotServiceError::CouldNotVerifySnapshot {
                digest: snapshot.certificate_hash.clone(),
                path: unpacked_path.clone(),
            }
            .into());
        }
        self.certificate_verifier
            .verify_certificate_chain(
                certificate,
                self.certificate_client.clone(),
                &genesis_verifier,
            )
            .await?;

        Ok(unpacked_path)
    }
}

#[cfg(test)]
mod tests {
    use mithril_common::messages::{SnapshotListItemMessage, SnapshotListMessage, SnapshotMessage};
    use mithril_common::test_utils::fake_data;

    use crate::aggregator_client::MockAggregatorHTTPClient;

    use super::super::mock::*;

    use super::*;

    fn get_snapshot_list_message() -> SnapshotListMessage {
        let item1 = SnapshotListItemMessage {
            digest: "digest-1".to_string(),
            beacon: fake_data::beacon(),
            certificate_hash: "certificate-hash-1".to_string(),
            size: 1024,
            created_at: "whatever".to_string(),
            locations: vec!["location-1.1".to_string(), "location-1.2".to_string()],
        };
        let item2 = SnapshotListItemMessage {
            digest: "digest-2".to_string(),
            beacon: fake_data::beacon(),
            certificate_hash: "certificate-hash-2".to_string(),
            size: 1024,
            created_at: "whatever".to_string(),
            locations: vec!["location-2.1".to_string(), "location-2.2".to_string()],
        };

        vec![item1, item2]
    }

    fn get_snapshot_message() -> SnapshotMessage {
        SnapshotMessage {
            digest: "digest-10".to_string(),
            beacon: fake_data::beacon(),
            certificate_hash: "certificate-hash-10".to_string(),
            size: 1024,
            created_at: "whatever".to_string(),
            locations: vec!["location-10.1".to_string(), "location-10.2".to_string()],
        }
    }

    #[tokio::test]
    async fn test_list_snapshots() {
        let mut http_client = MockAggregatorHTTPClient::new();
        http_client
            .expect_get_content()
            .returning(|_| Ok(serde_json::to_string(&get_snapshot_list_message()).unwrap()));
        let http_client = Arc::new(http_client);
        let snapshot_client = SnapshotClient::new(http_client.clone());
        let certificate_client = CertificateClient::new(http_client);
        let certificate_verifier = MockCertificateVerifierImpl::new();
        let snapshot_service = MithrilClientSnapshotService::new(
            Arc::new(snapshot_client),
            Arc::new(certificate_client),
            Arc::new(certificate_verifier),
        );

        let list = snapshot_service.list().await.unwrap();

        assert_eq!(2, list.len());
    }

    #[tokio::test]
    async fn test_list_snapshots_err() {
        let mut http_client = MockAggregatorHTTPClient::new();
        http_client
            .expect_get_content()
            .returning(|_| {
                Err(AggregatorHTTPClientError::RemoteServerUnreachable(
                    "whatever".to_string(),
                ))
            })
            .times(1);
        let http_client = Arc::new(http_client);
        let snapshot_client = SnapshotClient::new(http_client.clone());
        let certificate_client = CertificateClient::new(http_client);
        let certificate_verifier = MockCertificateVerifierImpl::new();
        let snapshot_service = MithrilClientSnapshotService::new(
            Arc::new(snapshot_client),
            Arc::new(certificate_client),
            Arc::new(certificate_verifier),
        );

        snapshot_service.list().await.unwrap_err();
    }

    #[tokio::test]
    async fn test_show_snapshot() {
        let mut http_client = MockAggregatorHTTPClient::new();
        http_client
            .expect_get_content()
            .return_once(|_| Ok(serde_json::to_string(&get_snapshot_message()).unwrap()))
            .times(1);
        let http_client = Arc::new(http_client);
        let snapshot_client = SnapshotClient::new(http_client.clone());
        let certificate_client = CertificateClient::new(http_client);
        let certificate_verifier = MockCertificateVerifierImpl::new();
        let snapshot_service = MithrilClientSnapshotService::new(
            Arc::new(snapshot_client),
            Arc::new(certificate_client),
            Arc::new(certificate_verifier),
        );

        assert_eq!(
            "digest-10".to_string(),
            snapshot_service.show("digest").await.unwrap().digest
        );
    }

    #[tokio::test]
    async fn test_show_snapshot_not_found() {
        let mut http_client = MockAggregatorHTTPClient::new();
        http_client
            .expect_get_content()
            .return_once(move |_| {
                Err(AggregatorHTTPClientError::RemoteServerLogical(
                    "whatever".to_string(),
                ))
            })
            .times(1);
        let http_client = Arc::new(http_client);
        let snapshot_client = SnapshotClient::new(http_client.clone());
        let certificate_client = CertificateClient::new(http_client);
        let certificate_verifier = MockCertificateVerifierImpl::new();
        let snapshot_service = MithrilClientSnapshotService::new(
            Arc::new(snapshot_client),
            Arc::new(certificate_client),
            Arc::new(certificate_verifier),
        );

        snapshot_service.show("digest-10").await.unwrap_err();
    }

    #[tokio::test]
    async fn test_show_snapshot_err() {
        let mut http_client = MockAggregatorHTTPClient::new();
        http_client
            .expect_get_content()
            .return_once(move |_| {
                Err(AggregatorHTTPClientError::ApiVersionMismatch(
                    "whatever".to_string(),
                ))
            })
            .times(1);
        let http_client = Arc::new(http_client);
        let snapshot_client = SnapshotClient::new(http_client.clone());
        let certificate_client = CertificateClient::new(http_client);
        let certificate_verifier = MockCertificateVerifierImpl::new();
        let snapshot_service = MithrilClientSnapshotService::new(
            Arc::new(snapshot_client),
            Arc::new(certificate_client),
            Arc::new(certificate_verifier),
        );

        snapshot_service.show("digest-10").await.unwrap_err();
    }

    #[tokio::test]
    async fn test_download_snapshot() {
        let mut http_client = MockAggregatorHTTPClient::new();
        http_client
            .expect_download()
            .return_once(move |_, _| Ok(()))
            .times(1);
        let http_client = Arc::new(http_client);
        let snapshot_client = SnapshotClient::new(http_client.clone());
        let certificate_client = CertificateClient::new(http_client);
        let certificate_verifier = MockCertificateVerifierImpl::new();
        let snapshot_service = MithrilClientSnapshotService::new(
            Arc::new(snapshot_client),
            Arc::new(certificate_client),
            Arc::new(certificate_verifier),
        );

        snapshot_service
            .download("digest", Path::new("pathdir"), "")
            .await
            .unwrap();
    }
}
