use ghost_brain::oracle::snapshot_engine::{
    DataSource, EventTsSource, PoolLifecycle, PoolMetrics, TransactionRecord, TxEvent,
};
use ghost_core::{EventSemanticEnvelope, EventTimeMetadata};
use seer::types::RawBytesMissingReason;
use solana_sdk::{pubkey::Pubkey, signature::Signature};

fn make_tx_event(event_time: EventTimeMetadata, legacy_timestamp_ms: u64) -> TxEvent {
    TxEvent {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: Pubkey::new_unique(),
        base_mint: Pubkey::new_unique(),
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics::default(),
        slot: Some(11),
        timestamp_ms: legacy_timestamp_ms,
        event_time,
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.5,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some(Signature::new_unique().to_string()),
        event_ordinal: Some(0),
        block_time: None,
        arrival_time_ms: Some(55),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    }
}

#[test]
fn transaction_record_prefers_ingress_wall_time_when_chain_time_is_absent() {
    let event = make_tx_event(EventTimeMetadata::new(None, Some(1_234), Some(55)), 9_999);

    let record = TransactionRecord::from_tx_event(&event);

    assert_eq!(record.timestamp_ms, 1_234);
    assert_eq!(record.event_ts_source, EventTsSource::IngressWall);
    assert_eq!(record.event_time.ingress_wall_ts_ms, Some(1_234));
}

#[test]
fn transaction_record_prefers_chain_time_over_legacy_timestamp() {
    let event = make_tx_event(
        EventTimeMetadata::new(Some(777), Some(1_234), Some(55)),
        9_999,
    );

    let record = TransactionRecord::from_tx_event(&event);

    assert_eq!(record.timestamp_ms, 777);
    assert_eq!(record.event_ts_source, EventTsSource::Event);
    assert_eq!(record.event_time.chain_event_ts_ms, Some(777));
}
