use crate::protocol::ToMessage;
use pallas_codec::utils::Int;
use pallas_primitives::{
    alonzo::{BoundedBytes, PlutusData},
    conway::{BigInt, Constr},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::str::Utf8Error;
use std::{collections::BTreeMap, fmt::Display};
use std::{fmt, ops::Deref};
use thiserror::Error;

/// The key of a ProtocolMessage
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProtocolMessagePartKey {
    /// The ProtocolMessage part key associated to the Snapshot Digest
    #[serde(rename = "snapshot_digest")]
    SnapshotDigest,

    /// The ProtocolMessage part key associated to the Cardano Transactions Merkle Root
    #[serde(rename = "cardano_transactions_merkle_root")]
    CardanoTransactionsMerkleRoot,

    /// The ProtocolMessage part key associated to the Next epoch aggregate verification key
    ///
    /// The AVK that will be allowed to be used to sign during the next epoch
    /// aka AVK(n-1)
    #[serde(rename = "next_aggregate_verification_key")]
    NextAggregateVerificationKey,

    /// The ProtocolMessage part key associated to the Next epoch protocol parameters
    ///
    /// The protocol parameters that will be allowed to be used to sign during the next epoch
    /// aka PPARAMS(n-1)
    #[serde(rename = "next_protocol_parameters")]
    NextProtocolParameters,

    /// The ProtocolMessage part key associated to the current epoch
    ///
    /// aka EPOCH(n)
    #[serde(rename = "current_epoch")]
    CurrentEpoch,

    /// The ProtocolMessage part key associated to the latest block number signed
    #[serde(rename = "latest_block_number")]
    LatestBlockNumber,

    /// The ProtocolMessage part key associated to the epoch for which the Cardano stake distribution is computed
    #[serde(rename = "cardano_stake_distribution_epoch")]
    CardanoStakeDistributionEpoch,

    /// The ProtocolMessage part key associated to the Cardano stake distribution Merkle root
    #[serde(rename = "cardano_stake_distribution_merkle_root")]
    CardanoStakeDistributionMerkleRoot,

    // Use a variant that takes a transaction ID
    /// The ProtocolMessage part key associated with a bridge transaction identified by its transaction ID
    #[serde(rename = "bridge_transaction")]
    BridgeTransaction(String), // The transaction ID is wrapped as a string
}

impl Display for ProtocolMessagePartKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::SnapshotDigest => write!(f, "snapshot_digest"),
            Self::NextAggregateVerificationKey => write!(f, "next_aggregate_verification_key"),
            Self::NextProtocolParameters => write!(f, "next_protocol_parameters"),
            Self::CurrentEpoch => write!(f, "current_epoch"),
            Self::CardanoTransactionsMerkleRoot => write!(f, "cardano_transactions_merkle_root"),
            Self::LatestBlockNumber => write!(f, "latest_block_number"),
            Self::CardanoStakeDistributionEpoch => write!(f, "cardano_stake_distribution_epoch"),
            Self::CardanoStakeDistributionMerkleRoot => {
                write!(f, "cardano_stake_distribution_merkle_root")
            }
            Self::BridgeTransaction(ref tx_id) => write!(f, "bridge_transaction: {}", tx_id),
        }
    }
}

/// Metadata for a bridge transaction.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct BridgeTransactionMetadata {
    /// The transaction ID.
    pub tx_id: String,

    /// The address of the sender.
    pub sender_address: String,

    /// The address of the recipient.
    pub recipient_address: String,

    /// The amount transferred in the transaction.
    pub amount: i64,
}

impl fmt::Display for BridgeTransactionMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BridgeTransactionMetadata {{ tx_id: {}, sender_address: {}, recipient_address: {}, amount: {} }}",
            self.tx_id, self.sender_address, self.recipient_address, self.amount
        )
    }
}

impl BridgeTransactionMetadata {
    /// Expected format: "sender_address: <address>, recipient_address: <address>, amount: <amount>"
    pub fn parse_str(input: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let parts: Vec<&str> = input.split(", ").collect();
        if parts.len() != 4 {
            return Err("Input string must contain exactly four parts".into());
        }

        let tx_id = parts[0]
            .strip_prefix("tx_id: ")
            .ok_or("Missing tx_id")?
            .to_string();

        let sender_address = parts[1]
            .strip_prefix("sender_address: ")
            .ok_or("Missing sender_address")?
            .to_string();

        let recipient_address = parts[2]
            .strip_prefix("recipient_address: ")
            .ok_or("Missing recipient_address")?
            .to_string();

        let amount_str = parts[3].strip_prefix("amount: ").ok_or("Missing amount")?;

        let amount: i64 = amount_str.parse().map_err(|_| "Invalid amount")?;

        Ok(BridgeTransactionMetadata {
            tx_id,
            sender_address,
            recipient_address,
            amount,
        })
    }
}
#[derive(Error, Debug)]
pub enum BridgeParseError {
    #[error("Invalid data structure for BridgeTransactionMetadata.")]
    InvalidStructure,
    #[error("Failed to decode UTF-8 string: {0}")]
    Utf8Error(#[from] Utf8Error),
    #[error("Expected integer value but found other data type.")]
    InvalidAmount,
}

// Note(hadelive): we can import the uplc library here
/// Trait for converting a struct to `PlutusData`.
pub trait ToPlutusData {
    /// Converts the struct to `PlutusData`.
    fn to_plutus_data(&self) -> PlutusData;
}

/// Trait for creating a struct from `PlutusData`.
pub trait FromPlutusData {
    /// Attempts to create a struct from `PlutusData`.
    ///
    /// # Errors
    ///
    /// Returns a `BridgeParseError` if the conversion fails.
    fn from_plutus_data(plutus_data: PlutusData) -> Result<Self, BridgeParseError>
    where
        Self: Sized;
}

impl ToPlutusData for BridgeTransactionMetadata {
    fn to_plutus_data(&self) -> PlutusData {
        PlutusData::Constr(Constr {
            tag: 121, // Set an appropriate tag
            any_constructor: None,
            fields: vec![
                PlutusData::BoundedBytes(BoundedBytes::from(
                    self.sender_address.as_bytes().to_vec(),
                )),
                PlutusData::BoundedBytes(BoundedBytes::from(
                    self.recipient_address.as_bytes().to_vec(),
                )),
                PlutusData::BigInt(BigInt::Int(Int::from(self.amount))),
            ],
        })
    }
}

impl FromPlutusData for BridgeTransactionMetadata {
    fn from_plutus_data(plutus_data: PlutusData) -> Result<Self, BridgeParseError> {
        let mut tx_metadata = BridgeTransactionMetadata {
            tx_id: String::new(),             // Default empty string
            sender_address: String::new(),    // Default empty string
            recipient_address: String::new(), // Default empty string
            amount: 0,                        // Default amount
        };
        if let PlutusData::Constr(a) = plutus_data {
            // Parse tx_id
            if let Some(PlutusData::BoundedBytes(b)) = a.fields.first() {
                tx_metadata.tx_id = std::str::from_utf8(b.deref())?.to_string();
            } else {
                return Err(BridgeParseError::InvalidStructure);
            }

            // Parse sender_address
            if let Some(PlutusData::BoundedBytes(b)) = a.fields.get(1) {
                tx_metadata.sender_address = std::str::from_utf8(b.deref())?.to_string();
            } else {
                return Err(BridgeParseError::InvalidStructure);
            }

            // Parse recipient_address
            if let Some(PlutusData::BoundedBytes(b)) = a.fields.get(2) {
                tx_metadata.recipient_address = std::str::from_utf8(b.deref())?.to_string();
            } else {
                return Err(BridgeParseError::InvalidStructure);
            }

            // Parse amount
            if let Some(PlutusData::BigInt(BigInt::Int(value))) = a.fields.get(3) {
                tx_metadata.amount = i128::from(*value) as i64;
            } else {
                return Err(BridgeParseError::InvalidAmount);
            }
        } else {
            return Err(BridgeParseError::InvalidStructure);
        }

        Ok(tx_metadata)
    }
}

/// The value of a ProtocolMessage
pub type ProtocolMessagePartValue = String;

/// ProtocolMessage represents a message that is signed (or verified) by the Mithril protocol
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProtocolMessage {
    /// Map of the messages combined into the digest
    /// aka MSG(p,n)
    pub message_parts: BTreeMap<ProtocolMessagePartKey, ProtocolMessagePartValue>,
}

impl ProtocolMessage {
    /// ProtocolMessage factory
    pub fn new() -> ProtocolMessage {
        ProtocolMessage {
            message_parts: BTreeMap::new(),
        }
    }

    /// Set the message part associated with a key
    /// Returns previously set value if it exists
    pub fn set_message_part(
        &mut self,
        key: ProtocolMessagePartKey,
        value: ProtocolMessagePartValue,
    ) -> Option<ProtocolMessagePartValue> {
        self.message_parts.insert(key, value)
    }

    /// Get the message part associated with a key
    pub fn get_message_part(
        &self,
        key: &ProtocolMessagePartKey,
    ) -> Option<&ProtocolMessagePartValue> {
        self.message_parts.get(key)
    }

    /// Computes the hash of the protocol message
    pub fn compute_hash(&self) -> String {
        let mut hasher = Sha256::new();
        self.message_parts.iter().for_each(|(k, v)| {
            hasher.update(k.to_string().as_bytes());
            hasher.update(v.as_bytes());
        });
        hex::encode(hasher.finalize())
    }
}

impl ToMessage for ProtocolMessage {
    fn to_message(&self) -> String {
        self.compute_hash()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pallas_primitives::alonzo::BoundedBytes;
    use pallas_primitives::conway::{BigInt, Constr, PlutusData};
    use pallas_primitives::Fragment;
    use std::ops::Deref;

    #[test]
    fn test_protocol_message_compute_hash_include_next_aggregate_verification_key() {
        let protocol_message = build_protocol_message_reference();
        let hash_expected = protocol_message.compute_hash();

        let mut protocol_message_modified = protocol_message.clone();
        protocol_message_modified.set_message_part(
            ProtocolMessagePartKey::NextAggregateVerificationKey,
            "next-avk-456".to_string(),
        );

        assert_ne!(hash_expected, protocol_message_modified.compute_hash());
    }

    #[test]
    fn test_protocol_message_compute_hash_include_snapshot_digest() {
        let protocol_message = build_protocol_message_reference();
        let hash_expected = protocol_message.compute_hash();

        let mut protocol_message_modified = protocol_message.clone();
        protocol_message_modified.set_message_part(
            ProtocolMessagePartKey::SnapshotDigest,
            "snapshot-digest-456".to_string(),
        );

        assert_ne!(hash_expected, protocol_message_modified.compute_hash());
    }

    #[test]
    fn test_protocol_message_compute_hash_include_cardano_transactions_merkle_root() {
        let protocol_message = build_protocol_message_reference();
        let hash_expected = protocol_message.compute_hash();

        let mut protocol_message_modified = protocol_message.clone();
        protocol_message_modified.set_message_part(
            ProtocolMessagePartKey::CardanoTransactionsMerkleRoot,
            "ctx-merke-root-456".to_string(),
        );

        assert_ne!(hash_expected, protocol_message_modified.compute_hash());
    }

    #[test]
    fn test_protocol_message_compute_hash_include_cardano_stake_distribution_epoch() {
        let protocol_message = build_protocol_message_reference();
        let hash_expected = protocol_message.compute_hash();

        let mut protocol_message_modified = protocol_message.clone();
        protocol_message_modified.set_message_part(
            ProtocolMessagePartKey::CardanoStakeDistributionEpoch,
            "cardano-stake-distribution-epoch-456".to_string(),
        );

        assert_ne!(hash_expected, protocol_message_modified.compute_hash());
    }

    #[test]
    fn test_protocol_message_compute_hash_include_cardano_stake_distribution_merkle_root() {
        let protocol_message = build_protocol_message_reference();
        let hash_expected = protocol_message.compute_hash();

        let mut protocol_message_modified = protocol_message.clone();
        protocol_message_modified.set_message_part(
            ProtocolMessagePartKey::CardanoStakeDistributionMerkleRoot,
            "cardano-stake-distribution-merkle-root-456".to_string(),
        );

        assert_ne!(hash_expected, protocol_message_modified.compute_hash());
    }

    #[test]
    fn test_protocol_message_compute_hash_include_lastest_immutable_file_number() {
        let protocol_message = build_protocol_message_reference();
        let hash_expected = protocol_message.compute_hash();

        let mut protocol_message_modified = protocol_message.clone();
        protocol_message_modified.set_message_part(
            ProtocolMessagePartKey::LatestBlockNumber,
            "latest-immutable-file-number-456".to_string(),
        );

        assert_ne!(hash_expected, protocol_message_modified.compute_hash());
    }

    #[test]
    fn test_protocol_message_compute_hash_include_next_protocol_parameters() {
        let protocol_message = build_protocol_message_reference();
        let hash_expected = protocol_message.compute_hash();

        let mut protocol_message_modified = protocol_message.clone();
        protocol_message_modified.set_message_part(
            ProtocolMessagePartKey::NextProtocolParameters,
            "latest-protocol-parameters-456".to_string(),
        );

        assert_ne!(hash_expected, protocol_message_modified.compute_hash());
    }

    #[test]
    fn test_protocol_message_compute_hash_the_same_hash_with_same_protocol_message() {
        assert_eq!(
            build_protocol_message_reference().compute_hash(),
            build_protocol_message_reference().compute_hash()
        );
    }

    fn build_protocol_message_reference() -> ProtocolMessage {
        let mut protocol_message = ProtocolMessage::new();
        protocol_message.set_message_part(
            ProtocolMessagePartKey::SnapshotDigest,
            "snapshot-digest-123".to_string(),
        );
        protocol_message.set_message_part(
            ProtocolMessagePartKey::NextAggregateVerificationKey,
            "next-avk-123".to_string(),
        );
        protocol_message.set_message_part(
            ProtocolMessagePartKey::NextProtocolParameters,
            "next-protocol-parameters-123".to_string(),
        );
        protocol_message.set_message_part(
            ProtocolMessagePartKey::CardanoTransactionsMerkleRoot,
            "ctx-merkle-root-123".to_string(),
        );
        protocol_message.set_message_part(
            ProtocolMessagePartKey::LatestBlockNumber,
            "latest-immutable-file-number-123".to_string(),
        );
        protocol_message.set_message_part(
            ProtocolMessagePartKey::CardanoStakeDistributionEpoch,
            "cardano-stake-distribution-epoch-123".to_string(),
        );
        protocol_message.set_message_part(
            ProtocolMessagePartKey::CardanoStakeDistributionMerkleRoot,
            "cardano-stake-distribution-merkle-root-123".to_string(),
        );

        let tx_metadata = BridgeTransactionMetadata {
            tx_id: "ddc3e082930982c541b3b3ce9046b8325253ce046784d57d83657cab3105a80b".to_string(),
            sender_address: "addr_test1qz0ugv82ruadcg8whwqnkfjfapwfxn49hsfa0dlmu2eyu5zsvjm8zc6dzn9c64p7w8wcadph53l0k3askg5g9pnvggxsvy935c".to_string(),
            recipient_address: "addr_test1qz0ugv82ruadcg8whwqnkfjfapwfxn49hsfa0dlmu2eyu5zsvjm8zc6dzn9c64p7w8wcadph53l0k3askg5g9pnvggxsvy935c".to_string(),
            amount: 10,
        };
        protocol_message.set_message_part(
            ProtocolMessagePartKey::BridgeTransaction(
                "ddc3e082930982c541b3b3ce9046b8325253ce046784d57d83657cab3105a80b".to_string(),
            ),
            tx_metadata.to_string(),
        );

        protocol_message
    }

    #[test]
    fn test_to_plutus() {
        let tx_metadata = BridgeTransactionMetadata {
            tx_id: "tx_id".to_string(),
            sender_address: "sender_address".to_string(),
            recipient_address: "recipient_address".to_string(),
            amount: 1000,
        };

        let plutus_data = tx_metadata.to_plutus_data();

        // Validate the structure of PlutusData
        match plutus_data {
            PlutusData::Constr(constr) => {
                assert_eq!(constr.tag, 121);
                assert_eq!(constr.fields.len(), 3);

                // Check sender address
                if let PlutusData::BoundedBytes(bounded_bytes) = &constr.fields[0] {
                    assert_eq!(
                        std::str::from_utf8(bounded_bytes.deref()).unwrap(),
                        "sender_address"
                    );
                } else {
                    panic!("Expected a BoundedBytes for sender_address");
                }

                // Check recipient address
                if let PlutusData::BoundedBytes(bounded_bytes) = &constr.fields[1] {
                    assert_eq!(
                        std::str::from_utf8(bounded_bytes.deref()).unwrap(),
                        "recipient_address"
                    );
                } else {
                    panic!("Expected a BoundedBytes for recipient_address");
                }

                // Check amount
                if let PlutusData::BigInt(bigint) = &constr.fields[2] {
                    if let BigInt::Int(value) = bigint {
                        assert_eq!(i128::from(*value), 1000);
                    } else {
                        panic!("Expected an Int for amount");
                    }
                } else {
                    panic!("Expected a BigInt for amount");
                }
            }
            _ => panic!("Expected PlutusData to be a Constr"),
        }
    }

    #[test]
    fn test_from_plutus() {
        let h = hex::decode(
            "d8799f4574785f69644e73656e6465725f6164647265737351726563697069656e745f616464726573731903e8ff",
        )
        .unwrap();
        let plutus_data = PlutusData::decode_fragment(&h).unwrap();
        // let plutus_data = PlutusData::Constr(Constr {
        //     tag: 121,
        //     any_constructor: None,
        //     fields: vec![
        //         PlutusData::BoundedBytes(BoundedBytes::from("tx_id".as_bytes().to_vec())),
        //         PlutusData::BoundedBytes(BoundedBytes::from("sender_address".as_bytes().to_vec())),
        //         PlutusData::BoundedBytes(BoundedBytes::from(
        //             "recipient_address".as_bytes().to_vec(),
        //         )),
        //         PlutusData::BigInt(BigInt::Int(Int::from(1000))),
        //     ],
        // });
        let x = plutus_data.encode_fragment().unwrap();
        println!("{}", hex::encode(x));

        let tx_metadata = BridgeTransactionMetadata::from_plutus_data(plutus_data).unwrap();

        // Validate the deserialized data
        assert_eq!(tx_metadata.sender_address, "sender_address");
        assert_eq!(tx_metadata.recipient_address, "recipient_address");
        assert_eq!(tx_metadata.amount, 1000);
    }

    #[test]
    fn test_invalid_plutus_data() {
        // Test for invalid data structures
        let invalid_data = PlutusData::Constr(Constr {
            tag: 0,
            any_constructor: None,
            fields: vec![PlutusData::BoundedBytes(BoundedBytes::from(
                "sender_address".as_bytes().to_vec(),
            ))],
        });

        let result = std::panic::catch_unwind(|| {
            let _ = BridgeTransactionMetadata::from_plutus_data(invalid_data).unwrap();
        });

        assert!(
            result.is_err(),
            "Expected panic on invalid PlutusData structure"
        );
    }

    #[test]
    fn test_parse_valid() {
        let input =
            "tx_id: ddc3e08293098..., sender_address: addr_test1qz0..., recipient_address: addr_test1qz1..., amount: 100";
        let result = BridgeTransactionMetadata::parse_str(input);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.tx_id, "ddc3e08293098...");
        assert_eq!(metadata.sender_address, "addr_test1qz0...");
        assert_eq!(metadata.recipient_address, "addr_test1qz1...");
        assert_eq!(metadata.amount, 100);
    }

    #[test]
    fn test_parse_invalid() {
        let input = "tx_id: ddc3e08293098...,sender_address: addr_test1qz0..., recipient_address: addr_test1qz1..."; // Missing amount
        let result = BridgeTransactionMetadata::parse_str(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_amount() {
        let input = "sender_address: addr_test1qz0..., recipient_address: addr_test1qz1..., amount: not_a_number";
        let result = BridgeTransactionMetadata::parse_str(input);
        assert!(result.is_err());
    }
}
