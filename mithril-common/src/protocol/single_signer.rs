use crate::{
    crypto_helper::ProtocolSigner,
    entities::{PartyId, SingleSignatures},
    protocol::ToMessage,
    StdResult,
};

/// The SingleSigner is the structure responsible for issuing SingleSignatures.
#[cfg_attr(test, derive(Debug))]
pub struct SingleSigner {
    party_id: PartyId,
    protocol_signer: ProtocolSigner,
}

impl SingleSigner {
    pub(super) fn new(party_id: PartyId, protocol_signer: ProtocolSigner) -> Self {
        Self {
            party_id,
            protocol_signer,
        }
    }

    /// Issue a single signature for the given message.
    ///
    /// If no lottery are won None will be returned.
    pub fn sign<T: ToMessage>(&self, message: &T) -> StdResult<Option<SingleSignatures>> {
        let signed_message = message.to_message();
        match self.protocol_signer.sign(signed_message.as_bytes()) {
            Some(signature) => {
                let won_indexes = signature.indexes.clone();

                Ok(Some(SingleSignatures::new(
                    self.party_id.to_owned(),
                    signature.into(),
                    won_indexes,
                )))
            }
            None => Ok(None),
        }
    }

    /// Return the partyId associated with this Signer.
    pub fn get_party_id(&self) -> PartyId {
        self.party_id.clone()
    }
}

#[cfg(test)]
mod test {
    use crate::{
        entities::{BridgeTransactionMetadata, ProtocolMessage, ProtocolMessagePartKey},
        protocol::SignerBuilder,
        test_utils::MithrilFixtureBuilder,
    };

    #[test]
    fn single_signer_should_be_able_to_issue_single_signature() {
        let fixture = MithrilFixtureBuilder::default().with_signers(3).build();
        let signers = fixture.signers_fixture();
        let signer = signers.first().unwrap();

        let (single_signer, _) = SignerBuilder::new(
            &fixture.signers_with_stake(),
            &fixture.protocol_parameters(),
        )
        .unwrap()
        .build_test_single_signer(
            signer.signer_with_stake.clone(),
            signer.kes_secret_key_path(),
        )
        .unwrap();

        println!(
            "protocol_message: {:#?}",
            build_protocol_message_reference()
        );
        // protocol_message: ProtocolMessage {
        //     message_parts: {
        //         SnapshotDigest: "snapshot-digest-123",
        //         CardanoTransactionsMerkleRoot: "ctx-merkle-root-123",
        //         NextAggregateVerificationKey: "next-avk-123",
        //         NextProtocolParameters: "next-protocol-parameters-123",
        //         LatestBlockNumber: "latest-immutable-file-number-123",
        //         CardanoStakeDistributionEpoch: "cardano-stake-distribution-epoch-123",
        //         CardanoStakeDistributionMerkleRoot: "cardano-stake-distribution-merkle-root-123",
        //         BridgeTransaction(
        //             "ddc3e082930982c541b3b3ce9046b8325253ce046784d57d83657cab3105a80b",
        //         ): "BridgeTransactionMetadata { tx_id: ddc3e082930982c541b3b3ce9046b8325253ce046784d57d83657cab3105a80b, sender_address: addr_test1qz0ugv82ruadcg8whwqnkfjfapwfxn49hsfa0dlmu2eyu5zsvjm8zc6dzn9c64p7w8wcadph53l0k3askg5g9pnvggxsvy935c, recipient_address: addr_test1qz0ugv82ruadcg8whwqnkfjfapwfxn49hsfa0dlmu2eyu5zsvjm8zc6dzn9c64p7w8wcadph53l0k3askg5g9pnvggxsvy935c, amount: 10 }",
        //     },
        // }
        let signature = single_signer
            .sign(&ProtocolMessage::default())
            .expect("Single signer should be able to issue single signature");

        //TODO(hadelive): more tests
        println!("signature: {:#?}", signature);
        // signature: Some(
        //     SingleSignatures {
        //         party_id: "pool1mxyec46067n3querj9cxkk0g0zlag93pf3ya9vuyr3wgkq2e6t7",
        //         won_indexes: [0, 3, 4, 5, 6, 12, 13, 16, 19, 20, 24, 25, 26, 29, 31, 33, 40, 41, 43, 46, 50, 52, 54, 56, 58, 61, 66, 67, 68, 69, 71, 72, 73, 75, 76, 77, 79, 81, 84, 85, 90, 93, 95, 96, 99],
        //         signature: ProtocolKey {
        //             key: StmSig {
        //                 sigma: Signature(
        //                     Signature {
        //                         point: blst_p1_affine {
        //                             x: blst_fp { l: [7468703594731719401, 10197996683465477009, 3082409773008296218, 6726611150551627533, 2934973867345584233, 114070374610589203] },
        //                             y: blst_fp { l: [2816661372522790803, 16737079611916699471, 16136221138828585368, 4999260321470034684, 1232514026492399476, 1538651617220385808] } }
        //                     }
        //                 ),
        //                 indexes: [0, 3, 4, 5, 6, 12, 13, 16, 19, 20, 24, 25, 26, 29, 31, 33, 40, 41, 43, 46, 50, 52, 54, 56, 58, 61, 66, 67, 68, 69, 71, 72, 73, 75, 76, 77, 79, 81, 84, 85, 90, 93, 95, 96, 99],
        //                 signer_index: 2
        //             }
        //         },
        //     },
        // )
        assert!(signature.is_some());
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
}
