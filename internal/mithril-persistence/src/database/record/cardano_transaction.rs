use sqlite::Row;

use mithril_common::entities::{
    BlockHash, BlockNumber, BridgeTransactionMetadata, CardanoTransaction, SlotNumber,
    TransactionHash,
};

use crate::database::Hydrator;
use crate::sqlite::{HydrationError, Projection, SqLiteEntity};

/// Cardano Transaction record is the representation of a cardano transaction.
#[derive(Debug, PartialEq, Clone)]
pub struct CardanoTransactionRecord {
    /// Unique hash of the transaction
    pub transaction_hash: TransactionHash,

    /// Block number of the transaction
    pub block_number: BlockNumber,

    /// Slot number of the transaction
    pub slot_number: SlotNumber,

    /// Block hash of the transaction
    pub block_hash: BlockHash,

    /// TODO(hadelive): consider separating these information into another table
    /// Bridge transaction metadata
    pub bridge_metadata: Option<BridgeTransactionMetadata>,
}

impl CardanoTransactionRecord {
    /// CardanoTransactionRecord factory
    pub fn new<T: Into<TransactionHash>, U: Into<BlockHash>>(
        hash: T,
        block_number: BlockNumber,
        slot_number: SlotNumber,
        block_hash: U,
        bridge_metadata: Option<BridgeTransactionMetadata>,
    ) -> Self {
        Self {
            transaction_hash: hash.into(),
            block_number,
            slot_number,
            block_hash: block_hash.into(),
            bridge_metadata,
        }
    }
}

impl From<CardanoTransaction> for CardanoTransactionRecord {
    fn from(transaction: CardanoTransaction) -> Self {
        Self {
            transaction_hash: transaction.transaction_hash,
            block_number: transaction.block_number,
            slot_number: transaction.slot_number,
            block_hash: transaction.block_hash,
            bridge_metadata: transaction.bridge_metadata,
        }
    }
}

impl From<CardanoTransactionRecord> for CardanoTransaction {
    fn from(other: CardanoTransactionRecord) -> CardanoTransaction {
        CardanoTransaction {
            transaction_hash: other.transaction_hash,
            block_number: other.block_number,
            slot_number: other.slot_number,
            block_hash: other.block_hash,
            bridge_metadata: other.bridge_metadata,
        }
    }
}

impl SqLiteEntity for CardanoTransactionRecord {
    fn hydrate(row: Row) -> Result<Self, HydrationError>
    where
        Self: Sized,
    {
        let transaction_hash = row.read::<&str, _>(0);
        let block_number = Hydrator::try_to_u64("cardano_tx.block_number", row.read::<i64, _>(1))?;
        let slot_number = Hydrator::try_to_u64("cardano_tx.slot_number", row.read::<i64, _>(2))?;
        let block_hash = row.read::<&str, _>(3);
        let bridge_metadata = match row.read::<Option<&str>, _>(4) {
            Some(metadata_str) => Some(
                BridgeTransactionMetadata::parse_str(metadata_str)
                    .map_err(|e| HydrationError::InvalidData(e.to_string()))?,
            ),
            None => None,
        };

        Ok(Self {
            transaction_hash: transaction_hash.to_string(),
            block_number: BlockNumber(block_number),
            slot_number: SlotNumber(slot_number),
            block_hash: block_hash.to_string(),
            bridge_metadata,
        })
    }

    fn get_projection() -> Projection {
        Projection::from(&[
            (
                "transaction_hash",
                "{:cardano_tx:}.transaction_hash",
                "text",
            ),
            ("block_number", "{:cardano_tx:}.block_number", "int"),
            ("slot_number", "{:cardano_tx:}.slot_number", "int"),
            ("block_hash", "{:cardano_tx:}.block_hash", "text"),
            ("bridge_metadata", "{:cardano_tx:}.bridge_metadata", "text"),
        ])
    }
}
