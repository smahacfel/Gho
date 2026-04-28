use std::collections::HashMap;

use seer::types::{transaction_event_time, GeyserEvent, RawBytesMissingReason};
use solana_sdk::signature::Signature;

fn make_tx_event(
    source: &str,
    event_ts_ms: Option<u64>,
    block_time: Option<i64>,
    arrival_ts_ms: Option<u64>,
) -> GeyserEvent {
    GeyserEvent::Transaction {
        slot: Some(42),
        event_ts_ms,
        arrival_ts_ms,
        event_time: ghost_core::EventTimeMetadata::default(),
        signature: Signature::new_unique(),
        accounts: vec![],
        instructions: vec![],
        logs: vec![],
        block_time,
        account_data: HashMap::new(),
        pre_balances: vec![],
        post_balances: vec![],
        success: true,
        error_code: None,
        compute_units_consumed: None,
        source: source.to_string(),
        synthetic: false,
        mpcf_payload_bytes: None,
        mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
        inner_instructions: vec![],
        pre_token_balances: vec![],
        post_token_balances: vec![],
    }
}

#[test]
fn grpc_without_block_time_uses_ingress_wall_not_chain() {
    let event = make_tx_event("grpc_global_stream", Some(1_234), None, Some(55));

    let event_time = transaction_event_time(&event);

    assert_eq!(event_time.chain_event_ts_ms, None);
    assert_eq!(event_time.ingress_wall_ts_ms, Some(1_234));
    assert_eq!(event_time.ingress_monotonic_ts_ms, Some(55));
}

#[test]
fn pumpportal_timestamp_does_not_claim_chain_time() {
    let event = make_tx_event("pumpportal", Some(9_876), Some(1_777_777_777), Some(88));

    let event_time = transaction_event_time(&event);

    assert_eq!(event_time.chain_event_ts_ms, None);
    assert_eq!(event_time.ingress_wall_ts_ms, None);
    assert_eq!(event_time.ingress_monotonic_ts_ms, Some(88));
}
