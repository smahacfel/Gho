use ghost_core::{EventSemanticEnvelope, EventTimeMetadata};
use ghost_launcher::components::seer::trade_event_to_pool_transaction;
use ghost_launcher::events::PoolTransaction;
use seer::types::{RawBytesMissingReason, TradeEvent};
use solana_sdk::{pubkey::Pubkey, signature::Signature};

fn make_trade(event_time: EventTimeMetadata, legacy_timestamp_ms: u64) -> TradeEvent {
    TradeEvent {
        semantic: EventSemanticEnvelope::default(),
        slot: Some(7),
        signature: Signature::new_unique(),
        event_ordinal: Some(0),
        tx_index: None,
        provenance: None,
        timestamp_ms: legacy_timestamp_ms,
        arrival_ts_ms: 55,
        event_time,
        pool_amm_id: Pubkey::new_unique(),
        mint: Pubkey::new_unique(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        is_dev_buy: false,
        amount: 123,
        max_sol_cost: 1_000_000_000,
        min_sol_output: 0,
        success: true,
        error_code: None,
        compute_units_consumed: None,
        owner_token_deltas: vec![],
        mpcf_payload: vec![],
        mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
        v_tokens_in_bonding_curve: Some(10.0),
        v_sol_in_bonding_curve: Some(5.0),
        market_cap_sol: None,
        global_config: None,
        fee_recipient: None,
        token_program: None,
        buy_variant: None,
        associated_bonding_curve: None,
        creator_vault: None,
        bonding_curve_v2: None,
        bonding_curve_v2_provenance: None,
        buy_remaining_accounts: Vec::new(),
        is_mayhem_mode: None,
        cu_price_micro_lamports: None,
        compute_unit_limit: None,
        inner_ix_count: None,
        cpi_depth: None,
        ata_create_count: None,
        signer_pre_balance_lamports: None,
        signer_post_balance_lamports: None,
        jito_tip_detected: None,
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
        curve_data_known: false,
        curve_finality: ghost_core::CurveFinality::Speculative,
        is_pumpswap: false,
    }
}

fn make_pool_tx(event_time: EventTimeMetadata, legacy_timestamp_ms: u64) -> PoolTransaction {
    PoolTransaction {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: Pubkey::new_unique().to_string(),
        slot: Some(9),
        event_ordinal: Some(0),
        tx_index: None,
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms: legacy_timestamp_ms,
        event_time,
        arrival_ts_ms: 77,
        signer: Pubkey::new_unique().to_string(),
        is_buy: true,
        volume_sol: 1.0,
        sol_amount_lamports: Some(1_000_000_000),
        token_amount_units: Some(123),
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Signature::new_unique().to_string(),
        success: true,
        error_code: None,
        compute_units_consumed: None,
        owner_token_deltas: vec![],
        mpcf_payload: vec![],
        mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
        token_mint: Some(Pubkey::new_unique().to_string()),
        v_tokens_in_bonding_curve: None,
        v_sol_in_bonding_curve: None,
        market_cap_sol: None,
        global_config: None,
        fee_recipient: None,
        token_program: None,
        buy_variant: None,
        associated_bonding_curve: None,
        creator_vault: None,
        bonding_curve_v2: None,
        bonding_curve_v2_provenance: None,
        buy_remaining_accounts: Vec::new(),
        is_mayhem_mode: None,
        cu_price_micro_lamports: None,
        compute_unit_limit: None,
        inner_ix_count: None,
        cpi_depth: None,
        ata_create_count: None,
        signer_pre_balance_lamports: None,
        signer_post_balance_lamports: None,
        jito_tip_detected: None,
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
        curve_data_known: false,
        curve_finality: ghost_core::CurveFinality::Speculative,
    }
}

#[test]
fn seer_trade_bridge_preserves_explicit_event_time() {
    let trade = make_trade(EventTimeMetadata::new(None, Some(1_234), Some(55)), 9_999);

    let pool_tx = trade_event_to_pool_transaction(&trade);

    assert_eq!(pool_tx.timestamp_ms, 9_999);
    assert_eq!(pool_tx.event_time.ingress_wall_ts_ms, Some(1_234));
    assert_eq!(pool_tx.compat_event_ts_ms(), Some(1_234));
}

#[test]
fn pool_transaction_prefers_chain_time_over_legacy_timestamp() {
    let pool_tx = make_pool_tx(
        EventTimeMetadata::new(Some(777), Some(1_234), Some(77)),
        9_999,
    );

    assert_eq!(pool_tx.compat_event_ts_ms(), Some(777));
    assert_eq!(pool_tx.effective_event_ts_ms(), Some(777));
}
