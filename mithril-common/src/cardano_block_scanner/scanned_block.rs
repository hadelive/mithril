use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
};

use pallas_primitives::conway::PseudoDatumOption;
use pallas_traverse::MultiEraBlock;

use crate::entities::{
    BlockNumber, BridgeTransactionMetadata, CardanoTransaction, ChainPoint, FromPlutusData,
    SlotNumber, TransactionHash,
};
/// A block scanned from a Cardano database
#[derive(Clone, PartialEq)]
pub struct ScannedBlock {
    /// Block hash
    pub block_hash: Vec<u8>,
    /// Block number
    pub block_number: BlockNumber,
    /// Slot number of the block
    pub slot_number: SlotNumber,
    /// Hashes of the transactions in the block
    pub transactions_hashes: Vec<TransactionHash>,

    /// Bridge transactions mapped by transaction hash
    pub bridge_transactions: HashMap<TransactionHash, BridgeTransactionMetadata>,
}

impl ScannedBlock {
    /// Scanned block factory
    pub fn new<B: Into<Vec<u8>>, T: Into<TransactionHash>>(
        block_hash: B,
        block_number: BlockNumber,
        slot_number: SlotNumber,
        transaction_hashes: Vec<T>,
        bridge_transactions: HashMap<TransactionHash, BridgeTransactionMetadata>,
    ) -> Self {
        Self {
            block_hash: block_hash.into(),
            block_number,
            slot_number,
            transactions_hashes: transaction_hashes.into_iter().map(|h| h.into()).collect(),
            bridge_transactions,
        }
    }

    pub(crate) fn convert(multi_era_block: MultiEraBlock) -> Self {
        let mut transactions = Vec::new();
        let mut bridge_transactions: HashMap<TransactionHash, BridgeTransactionMetadata> =
            HashMap::new();
        // TODO(hadelive): check bridge txs
        for tx in &multi_era_block.txs() {
            transactions.push(tx.hash().to_string());
            for o in tx.outputs() {
                // TODO(hadelive): read bridge_address from config
                if o.address().unwrap().to_string() == "bridge_address" {
                    let d = o.datum().unwrap();
                    let data = match d {
                        PseudoDatumOption::Data(x) => x.unwrap().unwrap(),
                        _ => continue,
                    };
                    // TODO(hadelive): need a function to parse datum(PlutusData/cbor) to struct
                    let datum = BridgeTransactionMetadata::from_plutus_data(data).unwrap();
                    bridge_transactions.insert(tx.hash().to_string(), datum);
                }
            }
        }

        Self::new(
            *multi_era_block.hash(),
            BlockNumber(multi_era_block.number()),
            SlotNumber(multi_era_block.slot()),
            transactions,
            bridge_transactions,
        )
    }

    /// Number of transactions in the block
    pub fn transactions_len(&self) -> usize {
        self.transactions_hashes.len()
    }

    /// Convert the scanned block into a list of Cardano transactions.
    ///
    /// Consume the block.
    /// TODO(hadelive): add bridge metadata
    pub fn into_transactions(self) -> Vec<CardanoTransaction> {
        let block_hash = hex::encode(&self.block_hash);
        self.transactions_hashes
            .into_iter()
            .map(|transaction_hash| {
                let metadata = self
                    .bridge_transactions
                    .get(&transaction_hash)
                    .cloned();
                CardanoTransaction::new(
                    transaction_hash,
                    self.block_number,
                    self.slot_number,
                    block_hash.clone(),
                    metadata,
                )
            })
            .collect::<Vec<_>>()
    }
}

impl Debug for ScannedBlock {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("ScannedBlock");
        debug
            .field("block_hash", &hex::encode(&self.block_hash))
            .field("block_number", &self.block_number)
            .field("slot_number", &self.slot_number)
            .field("transactions_hashes", &self.transactions_hashes)
            .finish()
    }
}

impl From<&ScannedBlock> for ChainPoint {
    fn from(scanned_block: &ScannedBlock) -> Self {
        ChainPoint::new(
            scanned_block.slot_number,
            scanned_block.block_number,
            hex::encode(&scanned_block.block_hash),
        )
    }
}
