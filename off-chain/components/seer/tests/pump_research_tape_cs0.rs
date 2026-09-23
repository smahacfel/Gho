//! CS0 proof for the only source-capture guarantee made by Pump Research Tape
//! V1.1: decoded protobuf schema-losslessness.  These tests intentionally do
//! not claim original gRPC wire-frame identity or preservation of unknown
//! protobuf fields.

use ghost_core::pump_research_tape::{
    PUMP_RESEARCH_PROST_VERSION_V1, PUMP_RESEARCH_SOURCE_CAPTURE_SEMANTICS_V1,
    PUMP_RESEARCH_SOURCE_CLIENT_VERSION_V1, PUMP_RESEARCH_SOURCE_PROTO_CRATE_VERSION_V1,
};
use prost::Message;
use std::fmt::Debug;
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, BlockHeight, CommitmentLevel, Message as GrpcMessage, Rewards,
    SubscribeUpdate, SubscribeUpdateAccount, SubscribeUpdateAccountInfo, SubscribeUpdateBlockMeta,
    SubscribeUpdateSlot, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
    Transaction as GrpcTransaction, TransactionStatusMeta, UnixTimestamp,
};

fn assert_schema_lossless_round_trip<M>(source: M)
where
    M: Message + Default + PartialEq + Debug,
{
    let encoded = source.encode_to_vec();
    let decoded = M::decode(encoded.as_slice()).expect("frozen-schema payload must decode");
    assert_eq!(decoded, source);
    assert_eq!(
        decoded.encode_to_vec(),
        encoded,
        "prost encoding is deterministic for this fixture"
    );
}

#[test]
fn frozen_dependency_versions_and_capture_claim_are_explicit() {
    assert_eq!(PUMP_RESEARCH_SOURCE_PROTO_CRATE_VERSION_V1, "1.14.2");
    assert_eq!(PUMP_RESEARCH_SOURCE_CLIENT_VERSION_V1, "1.15.4");
    assert_eq!(PUMP_RESEARCH_PROST_VERSION_V1, "0.12.6");
    assert_eq!(
        PUMP_RESEARCH_SOURCE_CAPTURE_SEMANTICS_V1,
        "decoded_protobuf_schema_lossless_v1"
    );
}

#[test]
fn deterministic_prost_round_trip_preserves_each_frozen_source_payload_family() {
    let transaction = SubscribeUpdateTransaction {
        transaction: Some(SubscribeUpdateTransactionInfo {
            signature: vec![1; 64],
            is_vote: false,
            transaction: Some(GrpcTransaction {
                signatures: vec![vec![1; 64]],
                message: Some(GrpcMessage {
                    header: None,
                    account_keys: vec![vec![2; 32]],
                    recent_blockhash: vec![3; 32],
                    instructions: vec![],
                    versioned: true,
                    address_table_lookups: vec![],
                }),
            }),
            meta: Some(TransactionStatusMeta {
                fee: 4,
                pre_balances: vec![5],
                post_balances: vec![6],
                log_messages: vec!["frozen-proto".to_owned()],
                ..TransactionStatusMeta::default()
            }),
            index: 0,
        }),
        slot: 42,
    };
    let account = SubscribeUpdateAccount {
        account: Some(SubscribeUpdateAccountInfo {
            pubkey: vec![2; 32],
            lamports: 3,
            owner: vec![4; 32],
            executable: false,
            rent_epoch: 5,
            data: vec![6, 7, 8],
            write_version: 9,
            txn_signature: Some(vec![10; 64]),
        }),
        slot: 43,
        is_startup: true,
    };
    let slot = SubscribeUpdateSlot {
        slot: 44,
        parent: Some(43),
        status: CommitmentLevel::Finalized as i32,
    };
    let block_meta = SubscribeUpdateBlockMeta {
        slot: 45,
        blockhash: "blockhash-v1".to_owned(),
        rewards: Some(Rewards { rewards: vec![] }),
        block_time: Some(UnixTimestamp { timestamp: 46 }),
        block_height: Some(BlockHeight { block_height: 47 }),
        parent_slot: 44,
        parent_blockhash: "parent-v1".to_owned(),
        executed_transaction_count: 11,
        entries_count: 12,
    };

    assert_schema_lossless_round_trip(transaction.clone());
    assert_schema_lossless_round_trip(account.clone());
    assert_schema_lossless_round_trip(slot.clone());
    assert_schema_lossless_round_trip(block_meta.clone());

    for update in [
        SubscribeUpdate {
            filters: vec!["transaction-filter".to_owned()],
            update_oneof: Some(UpdateOneof::Transaction(transaction)),
        },
        SubscribeUpdate {
            filters: vec!["account-filter".to_owned()],
            update_oneof: Some(UpdateOneof::Account(account)),
        },
        SubscribeUpdate {
            filters: vec!["slot-filter".to_owned()],
            update_oneof: Some(UpdateOneof::Slot(slot)),
        },
        SubscribeUpdate {
            filters: vec!["block-meta-filter".to_owned()],
            update_oneof: Some(UpdateOneof::BlockMeta(block_meta)),
        },
    ] {
        assert_schema_lossless_round_trip(update);
    }
}
