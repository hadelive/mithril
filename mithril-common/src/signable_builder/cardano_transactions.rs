use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;

use crate::{
    crypto_helper::{MKMap, MKMapNode, MKTreeNode, MKTreeStorer},
    entities::{
        BlockNumber, BlockRange, BridgeTransactionMetadata, ProtocolMessage, ProtocolMessagePartKey,
    },
    signable_builder::SignableBuilder,
    StdResult,
};

#[cfg(test)]
use mockall::automock;

/// Cardano transactions importer
#[cfg_attr(test, automock)]
#[async_trait]
pub trait TransactionsImporter: Send + Sync {
    /// Import all transactions up to the given beacon into the system
    async fn import(&self, up_to_beacon: BlockNumber) -> StdResult<()>;
}

/// Block Range Merkle roots retriever
#[cfg_attr(test, automock)]
#[async_trait]
pub trait BlockRangeRootRetriever<S: MKTreeStorer>: Send + Sync {
    /// Returns a Merkle map of the block ranges roots up to a given beacon
    async fn retrieve_block_range_roots<'a>(
        &'a self,
        up_to_beacon: BlockNumber,
    ) -> StdResult<Box<dyn Iterator<Item = (BlockRange, MKTreeNode)> + 'a>>;

    /// Returns a Merkle map of the block ranges roots up to a given beacon
    async fn compute_merkle_map_from_block_range_roots(
        &self,
        up_to_beacon: BlockNumber,
    ) -> StdResult<MKMap<BlockRange, MKMapNode<BlockRange, S>, S>> {
        let block_range_roots_iterator = self
            .retrieve_block_range_roots(up_to_beacon)
            .await?
            .map(|(block_range, root)| (block_range, root.into()));
        let mk_hash_map = MKMap::new_from_iter(block_range_roots_iterator)
        .with_context(|| "BlockRangeRootRetriever failed to compute the merkelized structure that proves ownership of the transaction")?;

        Ok(mk_hash_map)
    }
}

/// Transactions retriever
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TransactionsRetriever: Sync + Send {
    /// Get a list of transactions by hashes using chronological order
    async fn get_by_hashes(
        &self,
        hashes: Vec<crate::entities::TransactionHash>,
        up_to: BlockNumber,
    ) -> StdResult<Vec<crate::entities::CardanoTransaction>>;

    /// Get by block ranges
    async fn get_by_block_ranges(
        &self,
        block_ranges: Vec<BlockRange>,
    ) -> StdResult<Vec<crate::entities::CardanoTransaction>>;
}

/// A [CardanoTransactionsSignableBuilder] builder
pub struct CardanoTransactionsSignableBuilder<S: MKTreeStorer> {
    transaction_importer: Arc<dyn TransactionsImporter>,
    block_range_root_retriever: Arc<dyn BlockRangeRootRetriever<S>>,
    transaction_retriever: Arc<dyn TransactionsRetriever>,
}

impl<S: MKTreeStorer> CardanoTransactionsSignableBuilder<S> {
    /// Constructor
    pub fn new(
        transaction_importer: Arc<dyn TransactionsImporter>,
        block_range_root_retriever: Arc<dyn BlockRangeRootRetriever<S>>,
        transaction_retriever: Arc<dyn TransactionsRetriever>,
    ) -> Self {
        Self {
            transaction_importer,
            block_range_root_retriever,
            transaction_retriever,
        }
    }
}

#[async_trait]
impl<S: MKTreeStorer> SignableBuilder<BlockNumber> for CardanoTransactionsSignableBuilder<S> {
    // TODO(hadelive): update protocol message here
    async fn compute_protocol_message(&self, beacon: BlockNumber) -> StdResult<ProtocolMessage> {
        self.transaction_importer.import(beacon).await?;

        let mk_root = self
            .block_range_root_retriever
            .compute_merkle_map_from_block_range_roots(beacon)
            .await?
            .compute_root()?;

        let mut protocol_message = ProtocolMessage::new();
        // contains all the bridge txs in the block
        // TODO(hadelive): need to store all the bridge txs and the metadata
        protocol_message.set_message_part(
            ProtocolMessagePartKey::CardanoTransactionsMerkleRoot,
            mk_root.to_hex(),
        );
        protocol_message.set_message_part(
            ProtocolMessagePartKey::LatestBlockNumber,
            beacon.to_string(),
        );
        // Retrieve the bridge transactions for the current block
        let bridge_transactions: Vec<BridgeTransactionMetadata> = self
            .transaction_retriever
            .get_by_block_ranges(vec![BlockRange::from_block_number_and_length(
                beacon,
                BlockNumber(1),
            )
            .unwrap()])
            .await?
            .iter()
            .filter_map(|transaction| transaction.bridge_metadata.clone())
            .collect();

        // Iterate through each bridge transaction and add metadata to the protocol message
        for tx in bridge_transactions {
            // Create a ProtocolMessagePartKey for the bridge transaction using the transaction ID
            let bridge_tx_key = ProtocolMessagePartKey::BridgeTransaction(tx.tx_id.to_string());

            // Store the metadata in the protocol message
            protocol_message.set_message_part(
                bridge_tx_key,               // Use the key that contains the transaction ID
                serde_json::to_string(&tx)?, // Serialize the struct to a JSON string
            );
        }

        Ok(protocol_message)
    }
}

#[cfg(test)]
mod tests {

    use crate::{
        crypto_helper::MKTreeStoreInMemory, entities::CardanoTransaction,
        test_utils::CardanoTransactionsBuilder,
    };

    use super::*;

    fn compute_mk_map_from_transactions(
        transactions: Vec<CardanoTransaction>,
    ) -> MKMap<BlockRange, MKMapNode<BlockRange, MKTreeStoreInMemory>, MKTreeStoreInMemory> {
        MKMap::new_from_iter(transactions.iter().map(|tx| {
            (
                BlockRange::from_block_number(tx.block_number),
                MKMapNode::TreeNode(tx.transaction_hash.clone().into()),
            )
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn test_compute_signable() {
        // Arrange
        let block_number = BlockNumber(1453);
        // TODO(hadelive)
        let transaction_retriever = MockTransactionsRetriever::new();
        // transaction_retriever_mock_config(&mut transaction_retriever);
        let transactions = CardanoTransactionsBuilder::new().build_transactions(3);
        let mk_map = compute_mk_map_from_transactions(transactions.clone());
        let mut transaction_importer = MockTransactionsImporter::new();
        transaction_importer
            .expect_import()
            .return_once(move |_| Ok(()));
        let retrieved_transactions = transactions.clone();
        let mut block_range_root_retriever = MockBlockRangeRootRetriever::new();
        block_range_root_retriever
            .expect_compute_merkle_map_from_block_range_roots()
            .return_once(move |_| Ok(compute_mk_map_from_transactions(retrieved_transactions)));

        let cardano_transactions_signable_builder = CardanoTransactionsSignableBuilder::new(
            Arc::new(transaction_importer),
            Arc::new(block_range_root_retriever),
            Arc::new(transaction_retriever),
        );

        // Action
        let signable = cardano_transactions_signable_builder
            .compute_protocol_message(block_number)
            .await
            .unwrap();

        // Assert
        let mut signable_expected = ProtocolMessage::new();
        signable_expected.set_message_part(
            ProtocolMessagePartKey::CardanoTransactionsMerkleRoot,
            mk_map.compute_root().unwrap().to_hex(),
        );
        signable_expected.set_message_part(
            ProtocolMessagePartKey::LatestBlockNumber,
            format!("{}", block_number),
        );
        assert_eq!(signable_expected, signable);
    }

    #[tokio::test]
    async fn test_compute_signable_with_no_block_range_root_return_error() {
        let block_number = BlockNumber(50);
        let transaction_retriever = MockTransactionsRetriever::new();
        // TODO(hadelive)
        // transaction_retriever_mock_config(&mut transaction_retriever);
        let mut transaction_importer = MockTransactionsImporter::new();
        transaction_importer.expect_import().return_once(|_| Ok(()));
        let mut block_range_root_retriever = MockBlockRangeRootRetriever::new();
        block_range_root_retriever
            .expect_compute_merkle_map_from_block_range_roots()
            .return_once(move |_| Ok(compute_mk_map_from_transactions(vec![])));
        let cardano_transactions_signable_builder = CardanoTransactionsSignableBuilder::new(
            Arc::new(transaction_importer),
            Arc::new(block_range_root_retriever),
            Arc::new(transaction_retriever),
        );

        let result = cardano_transactions_signable_builder
            .compute_protocol_message(block_number)
            .await;

        assert!(result.is_err());
    }
}
