//! Seer ↔ Shadow Ledger authority boundary tests
//!
//! These tests enforce the architectural rule that:
//! - **Seer** is the canonical transaction *producer* (parse, dedup, mapping, event emission)
//! - **Shadow Ledger** is the authoritative curve-state *machine* (Gatekeeper + LivePipeline)
//!
//! The single, explicit bridge from Seer output to Shadow Ledger input is the
//! `trade_event_to_pool_transaction()` function in `ghost_launcher::components::seer`.
//!
//! Test categories:
//! - **A. Bridge/adapter tests** — verify all required semantic fields are preserved
//! - **B. Runtime routing tests** — verify the PoolTransaction has the right shape for downstream
//! - **C. Boundary regression tests** — verify Seer does NOT gain curve-state authority

// Expose the bridge function for testing by using the module path directly.
// ghost_launcher re-exports the components module.
use ghost_launcher::events::PoolTransaction;
use seer::types::{InstructionProvenance, RawBytesMissingReason, TradeEvent};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Build a minimal valid `TradeEvent` for use in bridge tests.
fn make_buy_trade(pool: Pubkey, mint: Pubkey) -> TradeEvent {
    TradeEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        slot: Some(42),
        signature: Signature::new_unique(),
        event_ordinal: Some(0),
        provenance: None,
        timestamp_ms: 1_700_000_000_000,
        arrival_ts_ms: 1_700_000_001_000,
        event_time: ghost_core::EventTimeMetadata::default(),
        pool_amm_id: pool,
        mint,
        signer: Pubkey::new_unique(),
        is_buy: true,
        is_dev_buy: false,
        amount: 1_000_000,           // 1M token units out
        max_sol_cost: 2_000_000_000, // 2 SOL in (lamports)
        min_sol_output: 0,
        success: true,
        error_code: None,
        compute_units_consumed: Some(50_000),
        owner_token_deltas: vec![],
        mpcf_payload: vec![0xde, 0xad],
        mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
        v_tokens_in_bonding_curve: Some(793_100_000.0),
        v_sol_in_bonding_curve: Some(32.5),
        market_cap_sol: Some(32.5),
        global_config: None,
        fee_recipient: None,
        token_program: None,
        buy_variant: None,
        associated_bonding_curve: None,
        is_mayhem_mode: None,
        cu_price_micro_lamports: Some(100_000),
        compute_unit_limit: Some(200_000),
        inner_ix_count: Some(4),
        cpi_depth: Some(2),
        ata_create_count: Some(1),
        signer_pre_balance_lamports: Some(5_000_000_000),
        signer_post_balance_lamports: None,
        jito_tip_detected: Some(false),
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
        curve_data_known: true,
        curve_finality: ghost_core::CurveFinality::Provisional,
        is_pumpswap: false,
    }
}

fn make_sell_trade(pool: Pubkey, mint: Pubkey) -> TradeEvent {
    let mut t = make_buy_trade(pool, mint);
    t.is_buy = false;
    t.max_sol_cost = 0;
    t.min_sol_output = 1_500_000_000; // 1.5 SOL out (lamports)
    t.amount = 800_000; // 800k token units in
    t
}

fn make_dev_buy_trade(pool: Pubkey, mint: Pubkey) -> TradeEvent {
    let mut t = make_buy_trade(pool, mint);
    t.is_dev_buy = true;
    t
}

/// Invoke the canonical bridge.
fn bridge(trade: &TradeEvent) -> PoolTransaction {
    ghost_launcher::components::seer::trade_event_to_pool_transaction(trade)
}

// ─── A. Bridge / adapter field-preservation tests ─────────────────────────────

/// A1: base mint is preserved in `token_mint`.
#[test]
fn test_bridge_preserves_base_mint() {
    let mint = Pubkey::new_unique();
    let trade = make_buy_trade(Pubkey::new_unique(), mint);
    let pool_tx = bridge(&trade);
    assert_eq!(
        pool_tx.token_mint,
        Some(mint.to_string()),
        "bridge must carry base_mint into token_mint"
    );
}

/// A2: pool (bonding_curve address) is preserved in `pool_amm_id`.
#[test]
fn test_bridge_preserves_pool_amm_id() {
    let pool = Pubkey::new_unique();
    let trade = make_buy_trade(pool, Pubkey::new_unique());
    let pool_tx = bridge(&trade);
    assert_eq!(
        pool_tx.pool_amm_id,
        pool.to_string(),
        "bridge must carry pool address into pool_amm_id"
    );
}

/// A3: tx ordering / timestamp metadata is preserved.
#[test]
fn test_bridge_preserves_ordering_metadata() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);
    assert_eq!(
        pool_tx.event_ordinal, trade.event_ordinal,
        "event_ordinal must be preserved"
    );
    assert_eq!(
        pool_tx.timestamp_ms, trade.timestamp_ms,
        "timestamp_ms must be preserved"
    );
    assert_eq!(
        pool_tx.arrival_ts_ms, trade.arrival_ts_ms,
        "arrival_ts_ms must be preserved"
    );
    assert_eq!(pool_tx.slot, trade.slot, "slot must be preserved");
    assert_eq!(
        pool_tx.signature,
        trade.signature.to_string(),
        "signature must be preserved"
    );
}

#[test]
fn test_bridge_preserves_provenance_metadata() {
    let mut trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    trade.provenance = Some(InstructionProvenance {
        outer_instruction_index: Some(4),
        inner_group_index: Some(2),
        outer_program_id: Some("outer-program".to_string()),
        invoked_program_id: "invoked-program".to_string(),
        stack_height: Some(3),
        from_cpi: true,
    });

    let pool_tx = bridge(&trade);

    assert_eq!(pool_tx.outer_instruction_index, Some(4));
    assert_eq!(pool_tx.inner_group_index, Some(2));
    assert_eq!(pool_tx.outer_program_id.as_deref(), Some("outer-program"));
    assert_eq!(pool_tx.cpi_stack_height, Some(3));
}

#[test]
fn test_bridge_default_mint_is_not_exposed_as_token_identity() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::default());
    let pool_tx = bridge(&trade);
    assert!(
        pool_tx.token_mint.is_none(),
        "default mint must not be forwarded as token identity"
    );
}

/// A4: buy side — SOL delta (max_sol_cost) is mapped to sol_amount_lamports.
#[test]
fn test_bridge_buy_side_sol_delta() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);
    assert!(pool_tx.is_buy, "is_buy must be preserved");
    assert_eq!(
        pool_tx.sol_amount_lamports,
        Some(trade.max_sol_cost),
        "buy: sol_amount_lamports must equal max_sol_cost"
    );
    assert_eq!(
        pool_tx.volume_sol,
        trade.max_sol_cost as f64 / 1_000_000_000.0,
        "buy: volume_sol must be max_sol_cost in SOL"
    );
}

/// A5: sell side — SOL delta (min_sol_output) is mapped to sol_amount_lamports.
#[test]
fn test_bridge_sell_side_sol_delta() {
    let trade = make_sell_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);
    assert!(!pool_tx.is_buy, "is_buy=false must be preserved for sells");
    assert_eq!(
        pool_tx.sol_amount_lamports,
        Some(trade.min_sol_output),
        "sell: sol_amount_lamports must equal min_sol_output"
    );
    assert_eq!(
        pool_tx.volume_sol,
        trade.min_sol_output as f64 / 1_000_000_000.0,
        "sell: volume_sol must be min_sol_output in SOL"
    );
}

/// A6: token delta (amount) is mapped to token_amount_units.
#[test]
fn test_bridge_token_delta() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);
    assert_eq!(
        pool_tx.token_amount_units,
        Some(trade.amount),
        "token_amount_units must equal trade.amount"
    );
}

/// A7: signer is preserved.
#[test]
fn test_bridge_preserves_signer() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);
    assert_eq!(
        pool_tx.signer,
        trade.signer.to_string(),
        "signer must be preserved"
    );
}

/// A7b: optional buy-path metadata is preserved for downstream override derivation.
#[test]
fn test_bridge_preserves_buy_path_metadata() {
    let mut trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let associated_bonding_curve = Pubkey::new_unique();
    trade.buy_variant = Some("legacy_buy".to_string());
    trade.associated_bonding_curve = Some(associated_bonding_curve);

    let pool_tx = bridge(&trade);

    assert_eq!(
        pool_tx.buy_variant.as_deref(),
        Some("legacy_buy"),
        "bridge must preserve observed buy_variant for downstream account overrides"
    );
    assert_eq!(
        pool_tx.associated_bonding_curve.as_deref(),
        Some(associated_bonding_curve.to_string().as_str()),
        "bridge must preserve associated_bonding_curve for downstream account overrides"
    );
}

/// A8: dev_buy flag and dev_buy_lamports are preserved for dev-buy trades.
#[test]
fn test_bridge_preserves_dev_buy_semantics() {
    let trade = make_dev_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);
    assert!(pool_tx.is_dev_buy, "is_dev_buy must be preserved");
    assert_eq!(
        pool_tx.dev_buy_lamports, trade.max_sol_cost,
        "dev_buy_lamports must equal max_sol_cost when is_dev_buy=true"
    );
}

/// A9: dev_buy_lamports is zero for non-dev buys.
#[test]
fn test_bridge_non_dev_buy_has_zero_dev_lamports() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    assert!(!trade.is_dev_buy);
    let pool_tx = bridge(&trade);
    assert!(!pool_tx.is_dev_buy);
    assert_eq!(
        pool_tx.dev_buy_lamports, 0,
        "non-dev buy must have dev_buy_lamports=0"
    );
}

/// A10: reserve fields (virtual reserves) are threaded through.
#[test]
fn test_bridge_preserves_reserve_fields() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);
    assert_eq!(
        pool_tx.reserve_base, trade.v_tokens_in_bonding_curve,
        "reserve_base must equal v_tokens_in_bonding_curve"
    );
    assert_eq!(
        pool_tx.reserve_quote, trade.v_sol_in_bonding_curve,
        "reserve_quote must equal v_sol_in_bonding_curve"
    );
}

/// A11: price_quote is derived from reserve ratio when both reserves are present.
#[test]
fn test_bridge_price_quote_derived_from_reserves() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    // Expect: v_sol / v_tokens = 32.5 / 793_100_000.0
    let pool_tx = bridge(&trade);
    let expected_price = 32.5_f64 / 793_100_000.0_f64;
    let actual = pool_tx
        .price_quote
        .expect("price_quote must be Some when both reserves are present");
    assert!(
        (actual - expected_price).abs() < 1e-20,
        "price_quote must equal v_sol/v_tokens; expected={} got={}",
        expected_price,
        actual
    );
}

/// A12: price_quote is None when v_tokens reserve is zero.
#[test]
fn test_bridge_price_quote_none_when_tokens_zero() {
    let mut trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    trade.v_tokens_in_bonding_curve = Some(0.0);
    let pool_tx = bridge(&trade);
    assert!(
        pool_tx.price_quote.is_none(),
        "price_quote must be None when v_tokens=0.0 (division by zero guard)"
    );
}

// ─── B. Runtime routing: shape for Shadow Ledger downstream path ───────────────

/// B1: sol_amount_lamports is Some — required for pool_tx_to_buffered_history_tx().
///
/// Shadow Ledger's Gatekeeper path calls `pool_tx_to_buffered_history_tx()` which
/// returns `None` (dropping the tx) if `sol_amount_lamports` is `None`.
/// The bridge must always populate this field.
#[test]
fn test_bridge_sol_amount_lamports_always_some() {
    for trade in [
        make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique()),
        make_sell_trade(Pubkey::new_unique(), Pubkey::new_unique()),
        make_dev_buy_trade(Pubkey::new_unique(), Pubkey::new_unique()),
    ] {
        let pool_tx = bridge(&trade);
        assert!(
            pool_tx.sol_amount_lamports.is_some(),
            "sol_amount_lamports must always be Some — Gatekeeper requires it"
        );
        assert!(
            pool_tx.sol_amount_lamports.unwrap() > 0,
            "sol_amount_lamports must be non-zero for a valid trade"
        );
    }
}

/// B2: token_amount_units is Some — required for pool_tx_to_buffered_history_tx().
#[test]
fn test_bridge_token_amount_units_always_some() {
    for trade in [
        make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique()),
        make_sell_trade(Pubkey::new_unique(), Pubkey::new_unique()),
    ] {
        let pool_tx = bridge(&trade);
        assert!(
            pool_tx.token_amount_units.is_some(),
            "token_amount_units must always be Some — Gatekeeper requires it"
        );
    }
}

/// B3: pool_amm_id is non-empty — required for per-pool routing in oracle_runtime.
#[test]
fn test_bridge_pool_amm_id_nonempty() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);
    assert!(
        !pool_tx.pool_amm_id.is_empty(),
        "pool_amm_id must be non-empty for routing"
    );
}

/// B4: buy PoolTransaction keeps `is_buy=true`, sell keeps `is_buy=false`.
///
/// `pool_tx_to_buffered_history_tx` maps `is_buy` → `TradeSide::Buy` / `TradeSide::Sell`.
/// If this mapping is lost the Gatekeeper builds wrong history.
#[test]
fn test_bridge_side_fidelity_for_shadow_ledger() {
    let buy = bridge(&make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique()));
    let sell = bridge(&make_sell_trade(Pubkey::new_unique(), Pubkey::new_unique()));
    assert!(buy.is_buy, "buy trade must produce is_buy=true");
    assert!(!sell.is_buy, "sell trade must produce is_buy=false");
}

// ─── C. Boundary regression tests ────────────────────────────────────────────

/// C1: the bridge function carries transaction *semantics*, not curve-state authority.
///
/// The PoolTransaction produced by `trade_event_to_pool_transaction` must NOT contain
/// a ready-to-apply state snapshot — it must only carry the raw transaction metadata
/// that Shadow Ledger's simulation will process.  Specifically the reserve fields are
/// *informational* (passed through for reference), not authoritative state.
///
/// This test ensures the bridge is a pure data mapping, not a state-evolution step.
#[test]
fn test_bridge_is_pure_mapping_no_side_effects() {
    let pool = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let trade = make_buy_trade(pool, mint);

    // Calling bridge twice must produce identical output — pure function, no state.
    let tx1 = bridge(&trade);
    let tx2 = bridge(&trade);
    assert_eq!(
        tx1.pool_amm_id, tx2.pool_amm_id,
        "bridge must be a pure, idempotent mapping"
    );
    assert_eq!(
        tx1.sol_amount_lamports, tx2.sol_amount_lamports,
        "bridge must be idempotent"
    );
    assert_eq!(
        tx1.token_amount_units, tx2.token_amount_units,
        "bridge must be idempotent"
    );
    assert_eq!(
        tx1.timestamp_ms, tx2.timestamp_ms,
        "bridge must be idempotent"
    );
}

/// C2: Seer does not carry curve-state authority — the PoolTransaction does NOT
/// contain a pre-applied bonding-curve snapshot.
///
/// After this PR, ShadowLedger curve state is owned by the Gatekeeper/LivePipeline
/// path only.  The bridge only carries deltas (d_sol, d_tok) and ordering metadata,
/// NOT a final reserve state that would implicitly commit a curve advance.
///
/// If someone later changes the bridge to pre-apply reserves, the reserve fields in
/// the output MUST still be the raw virtual reserves from the source trade (not a
/// post-advance simulation result).
#[test]
fn test_bridge_reserve_fields_are_informational_not_authoritative() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);

    // The bridge must carry reserves as-received from the parser, unchanged.
    assert_eq!(
        pool_tx.v_tokens_in_bonding_curve, trade.v_tokens_in_bonding_curve,
        "v_tokens must be passed through verbatim — not simulated or advanced"
    );
    assert_eq!(
        pool_tx.v_sol_in_bonding_curve, trade.v_sol_in_bonding_curve,
        "v_sol must be passed through verbatim — not simulated or advanced"
    );
    // reserve_base / reserve_quote (used by oracle scoring) equal the virtual reserves.
    assert_eq!(pool_tx.reserve_base, trade.v_tokens_in_bonding_curve);
    assert_eq!(pool_tx.reserve_quote, trade.v_sol_in_bonding_curve);
}

/// C3: The bridge output is consumed by Shadow Ledger, not the other way around.
///
/// This test documents (and locks in) the direction of the handoff:
/// Seer produces → PoolTransaction → Shadow Ledger consumes.
/// The PoolTransaction must carry enough information for the Gatekeeper to build
/// a `BufferedTx` and for LivePipeline to build a `LiveTxEvent`.
///
/// Required fields verified: pool_amm_id, is_buy, sol_amount_lamports,
/// token_amount_units, timestamp_ms, signer, signature.
#[test]
fn test_bridge_output_sufficient_for_shadow_ledger_gatekeeper() {
    let trade = make_buy_trade(Pubkey::new_unique(), Pubkey::new_unique());
    let pool_tx = bridge(&trade);

    // All fields required by pool_tx_to_buffered_history_tx() and pool_tx_to_tx_key()
    // in oracle_runtime.rs must be present and non-trivial.
    assert!(
        !pool_tx.pool_amm_id.is_empty(),
        "pool_amm_id required for routing"
    );
    assert!(
        pool_tx.sol_amount_lamports.is_some(),
        "sol_amount_lamports required by Gatekeeper"
    );
    assert!(
        pool_tx.token_amount_units.is_some(),
        "token_amount_units required by Gatekeeper"
    );
    assert!(
        pool_tx.timestamp_ms > 0,
        "timestamp_ms > 0 required for TxKey ordering"
    );
    assert!(
        !pool_tx.signer.is_empty(),
        "signer required for trader attribution"
    );
    assert!(
        !pool_tx.signature.is_empty(),
        "signature required for dedup"
    );
}
