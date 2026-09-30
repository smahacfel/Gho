use super::*;
use ghost_core::{EventSemanticEnvelope, EventTimeMetadata, RawProviderRoleV1};
use seer::types::{RawBytesMissingReason, TradeEvent};
use solana_sdk::signature::Signature;
fn make_trade(event_time: EventTimeMetadata, legacy_timestamp_ms: u64) -> TradeEvent {
    TradeEvent {
        metadata_availability: seer::types::TransactionMetadataAvailability {
            status_known: true,
            inner_instructions_known: true,
        },
        virtual_sol_reserves: None,
        virtual_token_reserves: None,
        real_sol_reserves: None,
        real_token_reserves: None,
        complete: None,
        semantic: EventSemanticEnvelope::default(),
        provider_id: None,
        provider_role: None,
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
        amm_observation: None,
    }
}

fn pool_event(mint: Pubkey, pool: Pubkey, at: u64, program: Pubkey) -> DetectedPoolEvent {
    use ghost_core::ingest_integrity::{
        ObservationProvenanceV1, ObservationSourceFamilyV1, ObservedPumpMutationV1,
        PumpMutationClaimsV1, PumpMutationFamilyV1,
    };
    let signature = Signature::new_unique();
    let candidate = serde_json::from_value(json!({"provider_role":"primary_authority",
        "signature":signature.to_string(),"amm_program_id":program,"pool_amm_id":pool,
        "base_mint":mint,"quote_mint":WSOL,"bonding_curve":pool,"creator":Pubkey::new_unique(),
        "timestamp":at,"slot":1,"token_total_supply":1_000_000_000_000_000u64}))
    .unwrap();
    DetectedPoolEvent {
        candidate,
        observation: Some(ObservedPumpMutationV1 {
            mutation_family: PumpMutationFamilyV1::InitializePool,
            signature,
            locator_hint: None,
            canonical_order: None,
            raw_transaction_mutation_count: Some(1),
            claims: PumpMutationClaimsV1 {
                success: Some(true),
                mint: Some(mint),
                curve: Some(pool),
                ..Default::default()
            },
            raw_provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
            provenance: ObservationProvenanceV1 {
                source_family: ObservationSourceFamilyV1::RawYellowstone,
                source_id: "test".into(),
                provider_id: "primary".into(),
                schema_id: "test".into(),
                payload_hash_blake3: [0; 32],
                received_at_monotonic_ns: at * 1_000_000,
            },
        }),
        runtime_disposition: Default::default(),
        continuity_observation_pool: None,
        detected_at: std::time::UNIX_EPOCH,
        sequence_number: 1,
        priority: seer::ipc::EventPriority::Normal,
    }
}
fn setup() -> (Gate0<Vec<u8>>, Pubkey, Pubkey, u64) {
    let now = seer::types::ingress_epoch_ms();
    let mint = Pubkey::new_unique();
    let pool = Pubkey::new_unique();
    let mut gate = Gate0::new(Gate0Config::default(), "test".into(), now, Vec::new()).unwrap();
    gate.on_pool(&pool_event(mint, pool, now, PUMP), now)
        .unwrap();
    (gate, mint, pool, now)
}
fn trade(mint: Pubkey, pool: Pubkey, at: u64) -> TradeEvent {
    let mut t = make_trade(EventTimeMetadata::new(None, Some(at), None), at);
    t.provider_role = Some(RawProviderRoleV1::PrimaryAuthority);
    t.pool_amm_id = pool;
    t.mint = mint;
    t.arrival_ts_ms = at;
    t.slot = Some(at / 400);
    t.tx_index = Some(0);
    t.virtual_sol_reserves = Some(40_000_000_000);
    t.virtual_token_reserves = Some(800_000_000_000_000);
    t.v_sol_in_bonding_curve = None;
    t.v_tokens_in_bonding_curve = None;
    t.amount = 2_000_000_000_000;
    t.max_sol_cost = 100_000_000;
    t.jito_tip_detected = Some(false);
    t.semantic.slot_quality = ghost_core::SlotQuality::Present;
    t
}
fn rows(gate: &Gate0<Vec<u8>>) -> Vec<Value> {
    std::str::from_utf8(&gate.writer)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
#[test]
fn failed_attempt_ratio_is_transaction_deduplicated_and_never_counts_as_volume() {
    let (mut g, m, p, t) = setup();
    let mut failed = trade(m, p, t + 100);
    failed.success = false;
    g.on_trade(&failed, t + 100).unwrap();
    g.on_trade(&failed, t + 101).unwrap();
    failed.event_ordinal = Some(1);
    g.on_trade(&failed, t + 102).unwrap();
    let success = trade(m, p, t + 200);
    g.on_trade(&success, t + 200).unwrap();
    g.tick(t + 30_000).unwrap();
    let r = rows(&g);
    let row = r.iter().find(|r| r["kind"] == "phase").unwrap();
    let metrics = &row["snapshot"]["metrics"];
    assert_eq!(metrics["failed_tx_ratio"], 0.5);
    assert_eq!(metrics["unique_failed_tx_count"], 1);
    assert_eq!(metrics["unique_attempted_tx_count"], 2);
    assert_eq!(metrics["buy_count"], 1);
    assert_eq!(metrics["total_volume_sol"], 0.1);
    assert_eq!(
        row["snapshot"]["quality"]["fields"]["failed_tx_ratio"]["status"],
        "available"
    );
    assert_eq!(r.last().unwrap()["reason"], "C");
}

#[test]
fn conflicting_redelivery_outcome_is_not_hidden_by_duplicate_gate() {
    for first_success in [true, false] {
        let (mut g, m, p, t) = setup();
        let mut tx = trade(m, p, t + 100);
        tx.success = first_success;
        tx.slot = Some(11);
        g.on_trade(&tx, t + 100).unwrap();
        // Redostawa nie może nadpisać pierwszego dowodu.
        g.on_trade(&tx, t + 101).unwrap();
        let retained = g.tokens[&m].retained_events;
        let entries = g.sessions.cross_pool_velocity_index().entry_count();
        tx.success = !first_success;
        tx.slot = Some(12);
        tx.signer = Pubkey::new_unique();
        let error = g.on_trade(&tx, t + 102).unwrap_err().to_string();
        assert!(error.contains("conflicting transaction outcome"));
        assert!(error.contains(&tx.signature.to_string()));
        assert_eq!(g.tokens[&m].retained_events, retained);
        assert_eq!(
            g.sessions.cross_pool_velocity_index().entry_count(),
            entries
        );
        let r = rows(&g);
        let conflict = r.last().unwrap();
        assert_eq!(conflict["kind"], "transaction_outcome_conflict");
        assert_eq!(conflict["first"]["success"], first_success);
        assert_eq!(conflict["conflicting"]["success"], !first_success);
        assert_eq!(conflict["first"]["received_ms"], t + 100);
        assert_eq!(conflict["first"]["slot"], 11);
        assert_eq!(conflict["conflicting"]["slot"], 12);
    }
}

#[test]
fn c_timer_records_before_eviction_and_exact_twenty_survives() {
    for count in [19u64, 20] {
        let (mut g, m, p, t) = setup();
        for i in 1..=count {
            g.on_trade(&trade(m, p, t + i * 100), t + i * 100).unwrap();
        }
        g.tick(t + 30_000).unwrap();
        let r = rows(&g);
        assert_eq!(r.iter().filter(|r| r["kind"] == "phase").count(), 1);
        assert_eq!(g.active_tokens(), usize::from(count == 20));
        if count == 19 {
            let last = r.last().unwrap();
            assert_eq!(last["reason"], "C");
            assert_eq!(last["gem"], false);
        }
    }
}
#[test]
fn d_and_e_have_distinct_exact_boundaries_and_cannot_wait_for_next_trade() {
    for count in [100u64, 101] {
        let (mut g, m, p, t) = setup();
        for i in 1..=20 {
            g.on_trade(&trade(m, p, t + i * 100), t + i * 100).unwrap();
        }
        for i in 21..=count {
            g.on_trade(&trade(m, p, t + 150_000 + i * 100), t + 150_000 + i * 100)
                .unwrap();
        }
        g.tick(t + 180_000).unwrap();
        assert_eq!(g.active_tokens(), 0);
        assert_eq!(
            rows(&g).last().unwrap()["reason"],
            if count == 100 { "D" } else { "E" }
        );
    }
    let (mut g, m, p, t) = setup();
    for i in 1..=20 {
        g.on_trade(&trade(m, p, t + i), t + i).unwrap();
    }
    for i in 1..=90 {
        g.on_trade(&trade(m, p, t + 179_000 + i), t + 179_000 + i)
            .unwrap();
    }
    g.tick(t + 180_000).unwrap();
    assert_eq!(g.active_tokens(), 1);
    g.tick(t + 209_000).unwrap();
    assert_eq!(g.active_tokens(), 1);
    g.tick(t + 210_000).unwrap();
    assert_eq!(rows(&g).last().unwrap()["reason"], "E");
}
#[test]
fn unknown_source_is_not_a_negative_label() {
    let (mut g, _, _, t) = setup();
    assert!(g.fail_source(t + 40_000, "source_gap").is_err());
    assert_eq!(g.summary.phase_counts, [0; 5]);
    assert!(rows(&g).last().unwrap()["gem"].is_null());
}
#[test]
fn five_cumulative_snapshots_are_complete_immutable_and_deduplicated() {
    let (mut g, m, p, t) = setup();
    let mut phase1 = None;
    for age in (250..=600_000).step_by(250) {
        let tx = trade(m, p, t + age);
        g.on_trade(&tx, t + age).unwrap();
        g.on_trade(&tx, t + age).unwrap();
        if age == 30_000 {
            g.tick(t + age).unwrap();
            phase1 = rows(&g).into_iter().find(|r| r["kind"] == "phase");
        }
    }
    g.tick(t + 600_000).unwrap();
    g.tick(t + 600_001).unwrap();
    assert_eq!(g.summary.phase_counts, [1; 5]);
    assert_eq!(g.active_tokens(), 0);
    let r = rows(&g);
    let phases: Vec<_> = r.iter().filter(|r| r["kind"] == "phase").collect();
    assert_eq!(Some(phases[0]), phase1.as_ref());
    for (i, row) in phases.iter().enumerate() {
        assert_eq!(row["age_ms"], PHASES_MS[i]);
        let metrics = row["snapshot"]["metrics"].as_object().unwrap();
        for name in metrics::REQUIRED {
            assert!(metrics.contains_key(*name), "{name}");
        }
        assert_eq!(metrics["tx_count"], PHASES_MS[i] / 250);
    }
    assert_eq!(r.last().unwrap()["reason"], "completed");
    assert_eq!(r.last().unwrap()["gem"], false);
}
#[test]
fn admission_closes_at_ten_hours_but_last_token_gets_full_lifetime() {
    let start = seer::types::ingress_epoch_ms();
    let mut g = Gate0::new(Gate0Config::default(), "tail".into(), start, Vec::new()).unwrap();
    let birth = start + 36_000_000 - 1;
    let m = Pubkey::new_unique();
    let p = Pubkey::new_unique();
    g.on_pool(&pool_event(m, p, birth, PUMP), birth).unwrap();
    g.on_pool(
        &pool_event(Pubkey::new_unique(), Pubkey::new_unique(), birth + 1, PUMP),
        birth + 1,
    )
    .unwrap();
    assert_eq!(g.summary.admitted, 1);
    assert!(!g.drained(birth + 1));
    for age in (250..=600_000).step_by(250) {
        g.on_trade(&trade(m, p, birth + age), birth + age).unwrap();
    }
    g.tick(birth + 600_000).unwrap();
    assert!(g.drained(birth + 600_000));
    assert_eq!(g.summary.phase_counts, [1; 5]);
}
#[test]
fn gem_uses_real_migration_age_and_post_sell_minimum_not_last_price() {
    assert_eq!(GEM_MIN_MARKET_CAP_SOL, 320);
    for migration_age in [3_000, 3_001] {
        let (mut g, m, _, t) = setup();
        let e = pool_event(m, canonical_amm(m), t + migration_age, AMM);
        g.on_pool(&e, t + migration_age).unwrap();
        let token = g.tokens.get_mut(&m).unwrap();
        let mut state = seer::amm_observation::AmmObservation {
            pool: canonical_amm(m),
            base_reserves: 1_000_000_000_000_000,
            quote_reserves: 320_000_000_000,
            pre_base: None,
            pre_quote: None,
            virtual_quote: 0,
            base_supply: Some(1_000_000_000_000_000),
            initialization: true,
        };
        token.observe_amm(&state, &e.candidate.signature);
        assert_eq!(token.label("completed"), Some(migration_age > 3_000));
        if migration_age > 3_000 {
            state.quote_reserves = 334_200_000_000;
            token.observe_amm(&state, "above-new-floor-below-old-floor");
            assert_eq!(token.label("completed"), Some(true));
            state.quote_reserves = 319_999_999_999;
            token.observe_amm(&state, "later-sell");
            state.quote_reserves = 400_000_000_000;
            token.observe_amm(&state, "later-recovery");
            assert_eq!(token.label("completed"), Some(false));
            assert!(token.post_migration_min_mc_sol.unwrap() < 320.0);
        }
        assert_eq!(rows(&g)[0]["gem_min_market_cap_sol"], 320);
    }
}
#[test]
fn pumpswap_migration_state_uses_canonical_amm_and_does_not_count_as_swap() {
    let (mut g, m, _, t) = setup();
    let migration_at = t + 3_001;
    let migration = pool_event(m, canonical_amm(m), migration_at, AMM);
    let migration_signature = migration.observation.as_ref().unwrap().signature.clone();
    g.on_pool(&migration, migration_at).unwrap();

    let mut init = trade(m, canonical_amm(m), migration_at + 1);
    init.is_pumpswap = true;
    init.is_dev_buy = true;
    init.signature = migration_signature;
    init.amm_observation = Some(seer::amm_observation::AmmObservation {
        pool: canonical_amm(m),
        base_reserves: 1_000_000_000_000_000,
        quote_reserves: 350_000_000_000,
        pre_base: None,
        pre_quote: None,
        virtual_quote: 0,
        base_supply: None,
        initialization: true,
    });
    g.on_trade(&init, migration_at + 1).unwrap();

    assert!(g.tokens[&m].migration_initial_seen);
    assert_eq!(g.summary.migration_initial_states, 1);
    assert_eq!(g.tokens[&m].swaps, 0);
    assert_eq!(g.tokens[&m].retained_events, 0);
    assert_eq!(g.tokens[&m].post_migration_min_mc_sol, Some(350.0));

    let mut swap = trade(m, canonical_amm(m), migration_at + 2);
    swap.is_pumpswap = true;
    swap.amm_observation = Some(seer::amm_observation::AmmObservation {
        pool: canonical_amm(m),
        base_reserves: 1_000_000_000_000_000,
        quote_reserves: 360_000_000_000,
        pre_base: Some(1_000_000_000_000_000),
        pre_quote: Some(350_000_000_000),
        virtual_quote: 0,
        base_supply: Some(1_000_000_000_000_000),
        initialization: false,
    });
    g.on_trade(&swap, migration_at + 2).unwrap();
    assert_eq!(g.tokens[&m].swaps, 1);
    assert_eq!(g.tokens[&m].retained_events, 1);
}

#[test]
fn arbitrary_amm_and_duplicate_migration_cannot_change_birth_or_migration_age() {
    let (mut g, m, _, t) = setup();
    g.on_pool(
        &pool_event(m, Pubkey::new_unique(), t + 3_001, AMM),
        t + 3_001,
    )
    .unwrap();
    assert!(g.tokens[&m].migration_ms.is_none());
    let e = pool_event(m, canonical_amm(m), t + 3_002, AMM);
    g.on_pool(&e, t + 3_002).unwrap();
    g.on_pool(&e, t + 5_000).unwrap();
    assert_eq!(g.tokens[&m].migration_ms, Some(t + 3_002));
    assert_eq!(g.tokens[&m].born_ms, t);
    assert_eq!(g.tokens[&m].label("completed"), None);
}
#[test]
fn early_interruption_does_not_manufacture_phase_five() {
    let (g, _, _, t) = setup();
    let log = g
        .close(
            t + 12_000,
            Some("interrupted"),
            Some("secondary_shutdown_failure"),
            json!({"probe":"ok"}),
        )
        .unwrap();
    let r: Vec<Value> = std::str::from_utf8(&log)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(r.iter().all(|r| r["kind"] != "phase"));
    assert!(r.iter().find(|r| r["kind"] == "terminal").unwrap()["gem"].is_null());
    let end = r.iter().find(|r| r["kind"] == "run_end").unwrap();
    assert_eq!(end["reason"], "interrupted");
    assert_eq!(end["shutdown_error"], "secondary_shutdown_failure");
    assert_eq!(end["runtime_diagnostics"]["probe"], "ok");
}

#[test]
fn c_counts_transactions_not_many_cpis_from_one_signature() {
    let (mut g, m, p, t) = setup();
    let signature = Signature::new_unique();
    for i in 1..=40u64 {
        let mut tx = trade(m, p, t + i);
        tx.signature = signature;
        tx.event_ordinal = Some(i as u32);
        g.on_trade(&tx, t + i).unwrap();
    }
    g.tick(t + 30_000).unwrap();
    let r = rows(&g);
    let last = r.last().unwrap();
    assert_eq!(last["successful_swap_count"], 40);
    assert_eq!(last["unique_successful_tx_count"], 1);
    assert_eq!(last["reason"], "C");
}

#[test]
fn jito_exact_anchor_deltas_are_retained_and_fingerprint_does_not_end_at_ten_seconds() {
    let (mut g, m, p, t) = setup();
    for age in (200..=600_000).step_by(200) {
        let mut tx = trade(m, p, t + age);
        tx.jito_tip_detected = Some(age >= 2_000);
        g.on_trade(&tx, t + age).unwrap();
    }
    g.tick(t + 600_000).unwrap();
    let r = rows(&g);
    let phases: Vec<_> = r.iter().filter(|r| r["kind"] == "phase").collect();
    let value =
        |phase: usize, name: &str| phases[phase]["snapshot"]["metrics"][name].as_f64().unwrap();
    assert!((value(0, "jito_tip_intensity") - 141.0 / 150.0).abs() < 1e-12);
    assert!((value(4, "jito_tip_intensity") - 2991.0 / 3000.0).abs() < 1e-12);
    assert!((value(4, "delta_jito_tip_intensity_1s_to_30s") - 141.0 / 150.0).abs() < 1e-12);
    assert!(
        (value(4, "delta_jito_tip_intensity_31s_to_300s") - (1491.0 / 1500.0 - 146.0 / 155.0))
            .abs()
            < 1e-12
    );
    assert_eq!(phases[0]["snapshot"]["metrics"]["failed_tx_ratio"], 0.0);
}

#[test]
fn current_native_sol_create_v2_with_default_quote_is_admitted() {
    let now = seer::types::ingress_epoch_ms();
    let mint = Pubkey::new_unique();
    let pool = Pubkey::new_unique();
    let mut event = pool_event(mint, pool, now, PUMP);
    event.candidate.quote_mint = Pubkey::default();
    event.candidate.creation_regime = ghost_core::PumpCreationRegimeV1 {
        schema_version: ghost_core::PumpCreationRegimeV1::SCHEMA_VERSION,
        creation_variant: ghost_core::PumpCreationVariantV1::CreateV2,
        quote_regime: ghost_core::PumpQuoteRegimeV1::NativeSol,
        quote_mint: Some(Pubkey::default().to_string()),
        mayhem_mode: ghost_core::PumpMayhemModeV1::False,
        provenance: ghost_core::PumpCreationRegimeProvenanceV1::CreateV2AndCreateEvent,
    };
    let mut gate = Gate0::new(Gate0Config::default(), "native".into(), now, Vec::new()).unwrap();
    gate.on_pool(&event, now).unwrap();
    assert_eq!(gate.summary.admitted, 1);
    assert_eq!(gate.active_tokens(), 1);
}

#[test]
fn default_quote_without_native_sol_birth_evidence_stays_fail_closed() {
    let now = seer::types::ingress_epoch_ms();
    let mint = Pubkey::new_unique();
    let pool = Pubkey::new_unique();
    let mut event = pool_event(mint, pool, now, PUMP);
    event.candidate.quote_mint = Pubkey::default();
    event.candidate.creation_regime = ghost_core::PumpCreationRegimeV1::default();
    let mut gate = Gate0::new(Gate0Config::default(), "unknown".into(), now, Vec::new()).unwrap();
    gate.on_pool(&event, now).unwrap();
    assert_eq!(gate.summary.admitted, 0);
    assert_eq!(gate.active_tokens(), 0);
    assert_eq!(
        gate.summary
            .pool_rejections
            .get("default_quote_without_native_sol_evidence"),
        Some(&1)
    );
}

#[test]
fn gate0_progress_reaches_session_index_and_gap_requires_fresh_continuity() {
    let t = seer::types::ingress_epoch_ms();
    let mut g = Gate0::new(Gate0Config::default(), "progress".into(), t, Vec::new()).unwrap();
    let index = g.sessions.cross_pool_velocity_index();
    let window = g.cpv_config.lookback_window_ms;
    let progress = |epoch, at, gap| seer::types::PrimaryTradeFeedProgressV1 {
        provider_id: "primary".into(),
        epoch,
        event_ms: at,
        received_ms: at,
        gap,
        gap_reason: None,
    };
    g.on_progress(&progress(1, t, false), t).unwrap();
    assert!(!index.is_ready());
    let at = t + window;
    g.on_progress(&progress(1, at, false), at).unwrap();
    assert!(index.is_ready());
    g.on_progress(&progress(2, at + 1, false), at + 1).unwrap();
    assert!(!index.is_ready());
    g.on_progress(&progress(2, at + 2, true), at + 2).unwrap();
    assert!(!index.is_ready());
    g.on_progress(&progress(1, at + 3, false), at + 3).unwrap();
    assert!(!index.is_ready());
    g.on_progress(&progress(2, at + 4, false), at + 4).unwrap();
    assert!(!index.is_ready());
    let ready = at + 4 + window;
    g.on_progress(&progress(2, ready, false), ready).unwrap();
    assert!(index.is_ready());
    let pool = Pubkey::new_unique();
    g.on_pool(&pool_event(Pubkey::new_unique(), pool, ready, PUMP), ready)
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &index,
        &g.sessions
            .get_session(&pool)
            .unwrap()
            .read()
            .cross_pool_velocity_index,
    ));
    // Luka po admission zamyka run; nie wolno odzyskać brakującej historii kohorty.
    assert!(g
        .on_progress(&progress(2, ready + 1, true), ready + 1)
        .is_err());
    assert!(!index.is_ready());
    assert_eq!(g.active_tokens(), 0);
    assert!(rows(&g)
        .iter()
        .any(|r| r["reason"] == "source_gap:unspecified"));
}

#[test]
fn unknown_developer_is_not_reported_as_verified_absence_of_dev_buy() {
    for known in [false, true] {
        let t = seer::types::ingress_epoch_ms();
        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let mut event = pool_event(mint, pool, t, PUMP);
        if !known {
            event.candidate.creator = Pubkey::default();
        }
        let mut g = Gate0::new(Gate0Config::default(), "identity".into(), t, Vec::new()).unwrap();
        g.on_pool(&event, t).unwrap();
        for age in 1..=20 {
            g.on_trade(&trade(mint, pool, t + age), t + age).unwrap();
        }
        g.tick(t + 30_000).unwrap();
        let r = rows(&g);
        let phase = r.iter().find(|r| r["kind"] == "phase").unwrap();
        let field = &phase["snapshot"]["quality"]["fields"]["dev_buyer_infrastructure_affinity"];
        assert!(phase["snapshot"]["metrics"]["dev_buyer_infrastructure_affinity"].is_null());
        if known {
            assert_eq!(field["status"], "not_applicable");
            assert!(field["reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("DBIA_NO_DEV_BUY")));
        } else {
            assert_eq!(field["status"], "input_unavailable");
            assert!(field["reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("verified_developer_identity_unavailable")));
            assert!(!field["reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("DBIA_NO_DEV_BUY")));
        }
    }
}

#[test]
fn phase_five_keeps_pumpswap_buyers_verified_prices_and_all_field_statuses() {
    let (mut g, m, p, t) = setup();
    let dev = g
        .sessions
        .get_session(&p)
        .unwrap()
        .read()
        .dev_wallet
        .unwrap();
    let buyers = [
        dev,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    ];
    let index = g.sessions.cross_pool_velocity_index();
    index.observe_source_progress(1, t - 300_000, t - 300_000, t - 300_000, &g.cpv_config);
    let mut fail = trade(m, p, t + 100);
    fail.success = false;
    g.on_trade(&fail, t + 100).unwrap();
    let migration = pool_event(m, canonical_amm(m), t + 10_000, AMM);
    for age in (200..=600_000).step_by(200) {
        let mut tx = trade(m, if age > 10_000 { canonical_amm(m) } else { p }, t + age);
        tx.signer = if age == 12_000 {
            dev
        } else {
            buyers[((age / 200 - 1) % 4) as usize]
        };
        tx.tx_index = Some((age % 400) as u32);
        tx.provenance = Some(seer::types::InstructionProvenance {
            outer_instruction_index: Some(0),
            inner_group_index: None,
            outer_program_id: None,
            invoked_program_id: if age > 10_000 { AMM } else { PUMP }.to_string(),
            stack_height: None,
            inner_instruction_path: Some(vec![]),
            from_cpi: false,
        });
        tx.toolchain_fingerprint = seer::types::ToolchainFingerprintInput {
            account_keys_len: Some(24),
            outer_instruction_count: Some(3),
            inner_instruction_group_count: Some(1),
            has_set_compute_unit_limit: Some(true),
            has_set_compute_unit_price: Some(true),
            external_fee_transfer_count: Some((age / 200 % 3) as u32),
            internal_fee_transfer_count: Some(0),
            filtered_wsol_self_transfer_count: Some(0),
        };
        tx.signer_pre_balance_lamports = Some(10_000_000_000);
        tx.signer_post_balance_lamports = Some(9_900_000_000);
        tx.is_buy = age % 2_000 != 0;
        tx.owner_token_deltas = vec![seer::types::TokenDelta {
            owner: tx.signer.to_string(),
            delta_raw: if tx.is_buy {
                i128::from(tx.amount)
            } else {
                -i128::from(tx.amount)
            },
            decimals: 6,
        }];
        tx.min_sol_output = 100_000_000;
        tx.inner_ix_count = Some(3);
        tx.cpi_depth = Some(2);
        tx.compute_unit_limit = Some(200_000);
        tx.cu_price_micro_lamports = Some(100);
        tx.jito_tip_detected = Some(age % 600 == 0);
        if age > 10_000 {
            tx.is_pumpswap = true;
            tx.virtual_sol_reserves = None;
            tx.virtual_token_reserves = None;
            let post_quote = 100_000_000_000 + age * 10_000;
            tx.amm_observation = Some(seer::amm_observation::AmmObservation {
                pool: canonical_amm(m),
                base_reserves: 200_000_000_000_000,
                quote_reserves: post_quote,
                pre_base: Some(200_000_000_000_000),
                pre_quote: Some(if tx.is_buy {
                    post_quote - 100_000_000
                } else {
                    post_quote + 100_000_000
                }),
                virtual_quote: 0,
                base_supply: Some(1_000_000_000_000_000),
                initialization: false,
            });
        }
        g.on_trade(&tx, t + age).unwrap();
        // An activity on the canonical AMM must not count as another market;
        // a genuinely different pool is the independent cross-pool witness.
        if age % 1_000 == 0 {
            index.observe_buy(
                "other-market",
                &buyers[0].to_string(),
                t + age,
                &g.cpv_config,
            );
        }
        if age == 10_000 {
            g.on_pool(&migration, t + age).unwrap();
            let mut init = tx.clone();
            init.is_pumpswap = true;
            init.is_dev_buy = true;
            init.pool_amm_id = canonical_amm(m);
            init.signature = migration.candidate.signature.parse().unwrap();
            init.amm_observation = Some(seer::amm_observation::AmmObservation {
                pool: canonical_amm(m),
                base_reserves: 200_000_000_000_000,
                quote_reserves: 100_000_000_000,
                pre_base: None,
                pre_quote: None,
                virtual_quote: 0,
                base_supply: Some(1_000_000_000_000_000),
                initialization: true,
            });
            g.on_trade(&init, t + age).unwrap();
        }
        g.on_progress(
            &seer::types::PrimaryTradeFeedProgressV1 {
                provider_id: "primary".into(),
                epoch: 1,
                event_ms: t + age,
                received_ms: t + age,
                gap: false,
                gap_reason: None,
            },
            t + age,
        )
        .unwrap();
        g.tick(t + age).unwrap();
    }
    let r = rows(&g);
    let phases: Vec<_> = r.iter().filter(|r| r["kind"] == "phase").collect();
    assert_eq!(phases.len(), 5);
    let last = &phases[4]["snapshot"];
    assert_eq!(last["venue"], "pumpswap");
    for phase in &phases {
        let snapshot = &phase["snapshot"];
        assert_eq!(snapshot["quality"]["fields"].as_object().unwrap().len(), 60);
        for name in metrics::REQUIRED {
            let value = &snapshot["metrics"][*name];
            let state = &snapshot["quality"]["fields"][*name];
            assert!(value.is_number() || value.is_null(), "{name}");
            assert!(state["status"].is_string(), "{name}");
            if value.is_null() {
                assert!(!state["reasons"].as_array().unwrap().is_empty(), "{name}");
            }
        }
    }
    for name in [
        "fee_topology_diversity_index_v2",
        "dev_buyer_infrastructure_affinity",
        "signer_cross_pool_velocity",
        "cpv_other_pool_activity",
        "demand_elasticity_score_v2",
        "single_tx_price_impact_pct",
        "single_sell_impact_pct_observed",
        "failed_tx_ratio",
    ] {
        assert!(
            last["metrics"][name].as_f64().is_some_and(f64::is_finite),
            "{name}: {}",
            last["quality"]
        );
    }
    assert_eq!(last["quality"]["developer"]["pubkey"], dev.to_string());
    for name in [
        "dev_buy_sol",
        "dev_tx_ratio",
        "dev_volume_ratio",
        "dev_paperhand_latency_ms",
    ] {
        assert!(
            last["metrics"][name].as_f64().is_some_and(f64::is_finite),
            "{name}"
        );
    }
    assert_eq!(last["metrics"]["signer_cross_pool_velocity"], 0.25);
    assert_eq!(last["quality"]["cpv"]["sample_count"], 4);
    assert_eq!(
        last["metrics"]["demand_elasticity_score"],
        last["metrics"]["demand_elasticity_score_v2"]
    );
    assert_eq!(
        last["quality"]["des_v2"]["price_source"],
        "verified_market_post_trade_price"
    );
    assert_eq!(
        last["quality"]["fields"]["bonding_progress_pct"]["status"],
        "not_applicable"
    );
    assert_eq!(r.last().unwrap()["reason"], "completed");
    assert_eq!(g.summary.completed_with_migration_initial_state, 1);
}

#[test]
fn early_top3_null_preserves_producer_reason_after_late_first_buy() {
    let (mut g, m, p, t) = setup();
    for i in 0..20 {
        g.on_trade(&trade(m, p, t + 4_000 + i), t + 4_000 + i)
            .unwrap();
    }
    g.tick(t + 30_000).unwrap();
    let r = rows(&g);
    let snapshot = &r.iter().find(|r| r["kind"] == "phase").unwrap()["snapshot"];
    assert!(snapshot["metrics"]["early_top3_buy_volume_pct_3s"].is_null());
    assert_eq!(
        snapshot["quality"]["fields"]["early_top3_buy_volume_pct_3s"]["reasons"],
        json!(["EARLY_TOP3_BUY_VOLUME_ZERO"])
    );
    assert_eq!(
        snapshot["quality"]["fields"]["early_top3_buy_volume_pct_3s"]["status"],
        "insufficient_sample"
    );
}

#[test]
fn invalid_reserves_do_not_reuse_previous_price_and_explain_null() {
    let (mut g, m, p, t) = setup();
    g.on_trade(&trade(m, p, t + 100), t + 100).unwrap();
    assert!(g.tokens[&m].last_market_cap_sol.is_some());
    let mut invalid = trade(m, p, t + 200);
    invalid.virtual_token_reserves = Some(0);
    g.on_trade(&invalid, t + 200).unwrap();
    g.tick(t + 30_000).unwrap();
    let r = rows(&g);
    let s = &r.iter().find(|r| r["kind"] == "phase").unwrap()["snapshot"];
    for name in ["market_cap_sol", "price_change_ratio"] {
        assert!(s["metrics"][name].is_null(), "{name}");
        assert_eq!(
            s["quality"]["fields"][name]["reasons"],
            json!(["invalid_reserves_or_supply"])
        );
    }
}

#[test]
fn missing_amm_state_explains_price_null_without_fabricating_price() {
    let (mut g, m, p, t) = setup();
    g.on_trade(&trade(m, p, t + 100), t + 100).unwrap();
    let mut amm = trade(m, canonical_amm(m), t + 200);
    amm.is_pumpswap = true;
    g.on_trade(&amm, t + 200).unwrap();
    g.tick(t + 30_000).unwrap();
    let r = rows(&g);
    let s = &r.iter().find(|r| r["kind"] == "phase").unwrap()["snapshot"];
    for name in ["market_cap_sol", "price_change_ratio"] {
        assert!(s["metrics"][name].is_null());
        assert_eq!(
            s["quality"]["fields"][name]["reasons"],
            json!(["missing_amm_state"])
        );
    }
}

#[test]
fn source_commitment_is_explicit_and_old_config_still_loads() {
    use seer::config::CommitmentLevel;
    let old: Gate0Config = toml::from_str("admission_ms = 1800000").unwrap();
    assert_eq!(old.source_commitment, CommitmentLevel::Confirmed);
    let profile: Gate0Config = toml::from_str(include_str!("../../../configs/gate0.toml")).unwrap();
    assert_eq!(profile.source_commitment, CommitmentLevel::Confirmed);
    let g = Gate0::new(profile, "commitment".into(), 1, Vec::new()).unwrap();
    let r = rows(&g);
    assert_eq!(r[0]["config"]["source_commitment"], "confirmed");
    assert_eq!(r[0]["gem_min_market_cap_sol"], 320);
}

#[test]
fn verified_create_pool_state_establishes_migration_before_pool_notice() {
    let (mut g, m, _, t) = setup();
    let mut init = trade(m, canonical_amm(m), t + 4_000);
    init.is_pumpswap = true;
    init.is_dev_buy = true;
    init.amm_observation = Some(seer::amm_observation::AmmObservation {
        pool: canonical_amm(m),
        base_reserves: 1_000_000_000_000_000,
        quote_reserves: 350_000_000_000,
        pre_base: None,
        pre_quote: None,
        virtual_quote: 0,
        base_supply: None,
        initialization: true,
    });
    // Nieudana transakcja nie dowodzi utworzenia puli.
    init.success = false;
    g.on_trade(&init, t + 4_000).unwrap();
    assert!(g.tokens[&m].migration_ms.is_none());
    init.success = true;
    init.signature = Signature::new_unique();
    g.on_trade(&init, t + 4_001).unwrap();
    assert_eq!(g.tokens[&m].migration_ms, Some(t + 4_001));
    assert!(g.tokens[&m].migration_initial_seen);
    assert_eq!(g.tokens[&m].last_market_cap_sol, Some(350.0));
    assert_eq!(g.tokens[&m].swaps, 0);
    assert_eq!(g.tokens[&m].retained_events, 0);
    assert!(g.tokens[&m].label_reasons.is_empty());
    g.on_trade(&init, t + 4_002).unwrap();
    g.on_pool(&pool_event(m, canonical_amm(m), t + 4_003, AMM), t + 4_003)
        .unwrap();
    assert_eq!(g.summary.migrations, 1);
    assert_eq!(g.summary.migration_initial_states, 1);
    assert_eq!(g.tokens[&m].migration_ms, Some(t + 4_001));
}

#[test]
fn ordinary_amm_swap_never_substitutes_for_create_pool_evidence() {
    let (mut g, m, _, t) = setup();
    let mut swap = trade(m, canonical_amm(m), t + 4_000);
    swap.is_pumpswap = true;
    swap.amm_observation = Some(seer::amm_observation::AmmObservation {
        pool: canonical_amm(m),
        base_reserves: 1_000_000_000_000_000,
        quote_reserves: 350_000_000_000,
        pre_base: None,
        pre_quote: None,
        virtual_quote: 0,
        base_supply: Some(1_000_000_000_000_000),
        initialization: false,
    });
    g.on_trade(&swap, t + 4_000).unwrap();
    assert!(g.tokens[&m].migration_ms.is_none());
    assert!(!g.tokens[&m].migration_initial_seen);
    assert!(g.tokens[&m].last_market_cap_sol.is_none());
    assert_eq!(g.summary.migrations, 0);
    assert_eq!(g.summary.migration_initial_states, 0);
    assert!(g.tokens[&m]
        .label_reasons
        .contains("missing_migration_create"));
}
