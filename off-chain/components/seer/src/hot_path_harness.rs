use std::{
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use ghost_core::{wal::Wal, RawProviderRoleV1};
use serde::Serialize;
use serde_json::{json, Value};
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use tempfile::tempdir;
use tokio::time::sleep;
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, CompiledInstruction, InnerInstruction, InnerInstructions,
    Message as GrpcMessage, SubscribeUpdate, SubscribeUpdateAccount, SubscribeUpdateAccountInfo,
    SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo, Transaction as GrpcTransaction,
    TransactionStatusMeta,
};

use crate::{
    binary_parser::{
        BinaryParser, ParsedTransactionBundle, DISC_BUY, DISC_CREATE, DISC_SELL, DISC_SWAP_BUY,
    },
    config::SeerConfig,
    grpc_connection::{
        pump_event_to_geyser_event, route_update_for_hot_path_harness,
        route_update_for_hot_path_harness_with_capture, DualLaneChannel, PumpEvent, TransportStats,
        GRPC_GLOBAL_STREAM_SOURCE_LABEL, PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID,
    },
    hot_path_metrics,
    ipc::{create_ipc_channel, BackpressurePolicy, EventPriority, IpcChannelConfig, IpcError},
    types::{GeyserEvent, InitializePoolEvent, RawBytesMissingReason, TradeEvent},
    Seer,
};

const HARNESS_ITERATIONS: usize = 200;
const BURST_EVENTS: usize = 3_072;
const BURST_BATCH_EVENTS: usize = 128;
const BURST_BATCH_INTERVAL: Duration = Duration::from_millis(50);
const QUEUE_DWELL_P99_SLA_NS: u64 = 250_000_000;
const QUEUE_OLDEST_EVENT_SLA_NS: u64 = 500_000_000;
const BASELINE_CANONICAL_PARITY_DIGEST: &str =
    "549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202";

#[derive(Clone, Copy, Debug)]
enum FixtureKind {
    PumpBuy,
    PumpSell,
    CreateAndInitialBuy,
    MultiplePumpMutations,
    PumpSwapInnerTrade,
}

fn deterministic_pubkey(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

fn deterministic_signature(seed: u8) -> Signature {
    Signature::from([seed; 64])
}

fn trade_data(discriminator: [u8; 8], amount: u64, sol_bound: u64) -> Vec<u8> {
    let mut data = discriminator.to_vec();
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&sol_bound.to_le_bytes());
    data
}

fn create_data() -> Vec<u8> {
    let mut data = DISC_CREATE.to_vec();
    for text in ["PR1B", "P1B", "https://example.invalid/pr1b.json"] {
        data.extend_from_slice(&(text.len() as u32).to_le_bytes());
        data.extend_from_slice(text.as_bytes());
    }
    data
}

fn pump_accounts() -> Vec<Pubkey> {
    let mut accounts: Vec<Pubkey> = (1_u8..=18).map(deterministic_pubkey).collect();
    accounts[8] =
        Pubkey::from_str(ghost_core::transaction_parser::ProgramIds::TOKEN_PROGRAM).unwrap();
    accounts[17] = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap();
    accounts
}

fn transaction_update(seed: u8, kind: FixtureKind) -> SubscribeUpdate {
    let signature = deterministic_signature(seed);
    let (accounts, instructions, inner_instructions) = match kind {
        FixtureKind::PumpBuy => {
            let accounts = pump_accounts();
            let instructions = vec![CompiledInstruction {
                program_id_index: 17,
                accounts: (0_u8..17).collect(),
                data: trade_data(DISC_BUY, 1_000_000, 50_000_000),
            }];
            (accounts, instructions, Vec::new())
        }
        FixtureKind::PumpSell => {
            let accounts = pump_accounts();
            let instructions = vec![CompiledInstruction {
                program_id_index: 17,
                accounts: (0_u8..17).collect(),
                data: trade_data(DISC_SELL, 750_000, 25_000_000),
            }];
            (accounts, instructions, Vec::new())
        }
        FixtureKind::CreateAndInitialBuy => {
            let accounts = pump_accounts();
            let instructions = vec![
                CompiledInstruction {
                    program_id_index: 17,
                    accounts: (0_u8..17).collect(),
                    data: create_data(),
                },
                CompiledInstruction {
                    program_id_index: 17,
                    accounts: (0_u8..17).collect(),
                    data: trade_data(DISC_BUY, 2_000_000, 100_000_000),
                },
            ];
            (accounts, instructions, Vec::new())
        }
        FixtureKind::MultiplePumpMutations => {
            let accounts = pump_accounts();
            let instructions = vec![
                CompiledInstruction {
                    program_id_index: 17,
                    accounts: (0_u8..17).collect(),
                    data: trade_data(DISC_BUY, 2_000_000, 100_000_000),
                },
                CompiledInstruction {
                    program_id_index: 17,
                    accounts: (0_u8..17).collect(),
                    data: trade_data(DISC_SELL, 500_000, 20_000_000),
                },
            ];
            (accounts, instructions, Vec::new())
        }
        FixtureKind::PumpSwapInnerTrade => {
            let wsol = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();
            let token_program =
                Pubkey::from_str(ghost_core::transaction_parser::ProgramIds::TOKEN_PROGRAM)
                    .unwrap();
            let pumpswap = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).unwrap();
            let accounts = vec![
                deterministic_pubkey(41),
                deterministic_pubkey(42),
                deterministic_pubkey(43),
                deterministic_pubkey(44),
                wsol,
                deterministic_pubkey(45),
                deterministic_pubkey(46),
                deterministic_pubkey(47),
                deterministic_pubkey(48),
                token_program,
                token_program,
                pumpswap,
                Pubkey::default(),
            ];
            let instructions = vec![CompiledInstruction {
                program_id_index: 12,
                accounts: vec![],
                data: vec![],
            }];
            let inner_instructions = vec![InnerInstructions {
                index: 0,
                instructions: vec![InnerInstruction {
                    program_id_index: 11,
                    accounts: (0_u8..=10).collect(),
                    data: DISC_SWAP_BUY.to_vec(),
                    stack_height: Some(2),
                }],
            }];
            (accounts, instructions, inner_instructions)
        }
    };

    let signature_bytes = signature.as_ref().to_vec();
    let tx = SubscribeUpdateTransaction {
        transaction: Some(SubscribeUpdateTransactionInfo {
            signature: signature_bytes.clone(),
            is_vote: false,
            transaction: Some(GrpcTransaction {
                signatures: vec![signature_bytes],
                message: Some(GrpcMessage {
                    header: None,
                    account_keys: accounts
                        .iter()
                        .map(|account| account.to_bytes().to_vec())
                        .collect(),
                    recent_blockhash: vec![seed; 32],
                    instructions,
                    versioned: false,
                    address_table_lookups: vec![],
                }),
            }),
            meta: Some(TransactionStatusMeta {
                pre_balances: vec![10_000_000_000; accounts.len()],
                post_balances: vec![9_900_000_000; accounts.len()],
                inner_instructions,
                ..TransactionStatusMeta::default()
            }),
            index: 0,
        }),
        slot: 10_000 + u64::from(seed),
    };

    SubscribeUpdate {
        filters: vec!["pr1b".to_string()],
        update_oneof: Some(UpdateOneof::Transaction(tx)),
    }
}

fn account_update(seed: u8) -> SubscribeUpdate {
    SubscribeUpdate {
        filters: vec!["pr1b-account".to_string()],
        update_oneof: Some(UpdateOneof::Account(SubscribeUpdateAccount {
            account: Some(SubscribeUpdateAccountInfo {
                pubkey: deterministic_pubkey(seed).to_bytes().to_vec(),
                lamports: 42,
                owner: deterministic_pubkey(seed.saturating_add(1))
                    .to_bytes()
                    .to_vec(),
                executable: false,
                rent_epoch: 0,
                data: vec![seed; 64],
                write_version: 0,
                txn_signature: Some(deterministic_signature(seed).as_ref().to_vec()),
            }),
            slot: 20_000 + u64::from(seed),
            is_startup: false,
        })),
    }
}

fn normalize_transaction(seed: u8, kind: FixtureKind) -> GeyserEvent {
    route_update_for_hot_path_harness(transaction_update(seed, kind))
        .expect("transaction fixture must normalize")
}

fn percentile(samples: &[u64], percentile: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let index = (sorted.len().saturating_sub(1) * percentile) / 100;
    sorted[index]
}

fn latency_summary(samples: &[u64]) -> Value {
    json!({
        "p50_ns": percentile(samples, 50),
        "p95_ns": percentile(samples, 95),
        "p99_ns": percentile(samples, 99),
    })
}

fn steady_state_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        let value = line.strip_prefix("VmRSS:")?;
        value.split_whitespace().next()?.parse::<u64>().ok()
    })
}

#[derive(Clone, Debug, Serialize)]
struct CanonicalParserParitySnapshotV1 {
    schema: &'static str,
    fixture: String,
    initialize_pool: Option<InitializePoolEvent>,
    trades: Vec<TradeEvent>,
}

fn canonical_parser_parity_snapshot(
    name: &str,
    bundle: &ParsedTransactionBundle,
) -> CanonicalParserParitySnapshotV1 {
    let mut initialize_pool = bundle.initialize_pool.clone();
    if let Some(pool) = initialize_pool.as_mut() {
        pool.event_ts_ms = pool.event_time.chain_event_ts_ms;
        pool.event_time.ingress_wall_ts_ms = None;
        pool.event_time.ingress_monotonic_ts_ms = None;
    }

    let mut trades = bundle.trades.clone();
    for trade in &mut trades {
        trade.timestamp_ms = trade.event_time.chain_event_ts_ms.unwrap_or_default();
        trade.arrival_ts_ms = 0;
        trade.event_time.ingress_wall_ts_ms = None;
        trade.event_time.ingress_monotonic_ts_ms = None;
    }

    CanonicalParserParitySnapshotV1 {
        schema: "canonical_parser_parity_snapshot_v1",
        fixture: name.to_string(),
        initialize_pool,
        trades,
    }
}

fn canonical_parity_digest(snapshots: &[CanonicalParserParitySnapshotV1]) -> String {
    blake3::hash(
        serde_json::to_vec(snapshots)
            .expect("canonical parser parity snapshot JSON")
            .as_slice(),
    )
    .to_hex()
    .to_string()
}

fn minimal_account_event() -> GeyserEvent {
    route_update_for_hot_path_harness(account_update(77)).expect("account fixture must normalize")
}

fn queued_account_event(account_event: &GeyserEvent, slot: u64) -> PumpEvent {
    PumpEvent::AccountUpdate {
        provider_id: Some("pr1b-primary".to_string()),
        provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
        pubkey: deterministic_pubkey(slot as u8).to_string(),
        slot,
        received_at: Instant::now(),
        decoded: Some(account_event.clone()),
    }
}

fn queued_transaction_event(seed: u8, kind: FixtureKind) -> PumpEvent {
    let update = transaction_update(seed, kind);
    let Some(UpdateOneof::Transaction(decoded)) = update.update_oneof else {
        panic!("transaction workload fixture");
    };
    PumpEvent::Transaction {
        provider_id: Some("pr1b-primary".to_string()),
        provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
        signature: deterministic_signature(seed).to_string(),
        slot: decoded.slot,
        received_at: Instant::now(),
        decoded: Box::new(decoded),
        capture_payload: false,
    }
}

fn queue_burst_measurement() -> (Value, GeyserEvent) {
    let capacity = SeerConfig::default_ingress_queue_capacity();
    assert_ne!(
        BURST_EVENTS, capacity,
        "capacity evidence must not define workload size"
    );
    assert_eq!(
        BURST_EVENTS % BURST_BATCH_EVENTS,
        0,
        "frozen operational workload must contain complete batches"
    );
    let (channel, receiver) = DualLaneChannel::with_capacities(capacity, 0);
    let stats = Arc::new(TransportStats::default());
    let account_event = minimal_account_event();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let consumer_barrier = Arc::clone(&barrier);
    let high_water = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let producer_high_water = Arc::clone(&high_water);

    let consumer = std::thread::spawn(move || {
        let parser = BinaryParser::new(false);
        let mut dwell_ns = Vec::with_capacity(BURST_EVENTS);
        consumer_barrier.wait();
        let started = Instant::now();
        for _ in 0..BURST_EVENTS {
            let event = receiver.queue.recv().expect("frozen burst event");
            dwell_ns.push(
                event
                    .received_at()
                    .elapsed()
                    .as_nanos()
                    .min(u128::from(u64::MAX)) as u64,
            );
            let normalized =
                pump_event_to_geyser_event(event, GRPC_GLOBAL_STREAM_SOURCE_LABEL, None)
                    .expect("transaction event mapping")
                    .expect("transaction normalization");
            parser
                .parse_transaction_bundle(&normalized)
                .expect("real parser consumer");
        }
        (started.elapsed(), dwell_ns)
    });

    barrier.wait();
    let producer_started = Instant::now();
    let mut peak_batch_ingress_events_per_second = 0.0_f64;
    for batch_start in (0..BURST_EVENTS).step_by(BURST_BATCH_EVENTS) {
        let batch_end = (batch_start + BURST_BATCH_EVENTS).min(BURST_EVENTS);
        let batch_started = Instant::now();
        for index in batch_start..batch_end {
            let kind = match index % 5 {
                0 => FixtureKind::PumpBuy,
                1 => FixtureKind::PumpSell,
                2 => FixtureKind::CreateAndInitialBuy,
                3 => FixtureKind::MultiplePumpMutations,
                _ => FixtureKind::PumpSwapInnerTrade,
            };
            assert!(
                channel.send(queued_transaction_event(index as u8, kind), &stats),
                "configured ingress capacity must absorb the frozen operational workload"
            );
            producer_high_water.fetch_max(channel.depth(), std::sync::atomic::Ordering::Relaxed);
        }
        let batch_events = batch_end - batch_start;
        peak_batch_ingress_events_per_second = peak_batch_ingress_events_per_second
            .max(batch_events as f64 / batch_started.elapsed().as_secs_f64().max(f64::EPSILON));
        if batch_end < BURST_EVENTS {
            std::thread::sleep(BURST_BATCH_INTERVAL);
        }
    }
    let producer_elapsed = producer_started.elapsed();
    let (consumer_elapsed, dwell_ns) = consumer.join().expect("concurrent parser consumer");
    let high_water = high_water.load(std::sync::atomic::Ordering::Relaxed);
    let operational_ingress_events_per_second =
        BURST_EVENTS as f64 / producer_elapsed.as_secs_f64().max(f64::EPSILON);
    let sustained_drain_events_per_second =
        BURST_EVENTS as f64 / consumer_elapsed.as_secs_f64().max(f64::EPSILON);
    let queue_dwell_p99_ns = percentile(&dwell_ns, 99);
    let oldest_event_age_ns = dwell_ns.iter().copied().max().unwrap_or_default();
    let missing_events = stats
        .msgs_overflow_dropped
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        missing_events, 0,
        "frozen operational workload must not open an ingress coverage gap"
    );
    assert!(
        queue_dwell_p99_ns <= QUEUE_DWELL_P99_SLA_NS,
        "queue dwell p99 {queue_dwell_p99_ns} ns exceeds SLA {QUEUE_DWELL_P99_SLA_NS} ns"
    );
    assert!(
        oldest_event_age_ns <= QUEUE_OLDEST_EVENT_SLA_NS,
        "oldest event age {oldest_event_age_ns} ns exceeds SLA {QUEUE_OLDEST_EVENT_SLA_NS} ns"
    );

    (
        json!({
            "input_events": BURST_EVENTS,
            "batch_events": BURST_BATCH_EVENTS,
            "batch_interval_ms": BURST_BATCH_INTERVAL.as_millis(),
            "configured_capacity": capacity,
            "queue_high_water": high_water,
            "oldest_event_age_ns": oldest_event_age_ns,
            "queue_dwell_p99_ns": queue_dwell_p99_ns,
            "queue_dwell_p99_sla_ns": QUEUE_DWELL_P99_SLA_NS,
            "oldest_event_age_sla_ns": QUEUE_OLDEST_EVENT_SLA_NS,
            "dwell_sla_passed": true,
            "peak_batch_ingress_events_per_second": peak_batch_ingress_events_per_second,
            "operational_ingress_events_per_second": operational_ingress_events_per_second,
            "sustained_drain_events_per_second": sustained_drain_events_per_second,
            "backlog_growth_events": high_water,
            "spilled_events": stats.msgs_spilled.load(std::sync::atomic::Ordering::Relaxed),
            "missing_events": missing_events,
            "concurrent_real_parser_consumer": true,
        }),
        account_event,
    )
}

fn saturation_measurement(account_event: GeyserEvent) -> Value {
    let (channel, receiver) = DualLaneChannel::with_capacities(1, 0);
    let stats = Arc::new(TransportStats::default());

    assert!(channel.send(queued_account_event(&account_event, 1), &stats));
    assert!(!channel.send(queued_account_event(&account_event, 2), &stats));
    let nonblocking_started = Instant::now();
    assert!(!channel.send(queued_account_event(&account_event, 3), &stats));
    let nonblocking_elapsed_ns = nonblocking_started.elapsed().as_nanos() as u64;
    let _ = receiver.queue.recv().expect("ingress event");
    assert!(channel.send(queued_account_event(&account_event, 4), &stats));
    let gap = receiver
        .take_completed_local_gap()
        .expect("one completed local gap");

    json!({
        "queue_capacity": 1,
        "blocked_before_drain": false,
        "blocking_wait_ns": 0,
        "nonblocking_send_ns": nonblocking_elapsed_ns,
        "local_gap_count": 1,
        "gap_id_blake3": bs58::encode(gap.gap_id_blake3).into_string(),
        "gap_reason": gap.reason.as_str(),
        "gap_recovered": gap.recovered,
        "explicit_missing_event_count": gap.missing_event_count,
        "first_dropped": gap.first_dropped,
        "last_dropped": gap.last_dropped,
        "silent_drop_count": 0,
    })
}

#[test]
fn pr1b_single_pass_live_transaction_contract() {
    hot_path_metrics::reset();
    let parser = BinaryParser::new(false);
    let event = route_update_for_hot_path_harness_with_capture(
        transaction_update(1, FixtureKind::PumpBuy),
        false,
    )
    .expect("normalize live fixture without capture");
    let bundle = parser
        .parse_transaction_bundle(&event)
        .expect("single-pass bundle");
    assert_eq!(bundle.trades.len(), 1);

    let counts = hot_path_metrics::snapshot();
    assert_eq!(counts.live_transaction_prost_encodes, 0);
    assert_eq!(counts.live_transaction_normalizer_decodes, 0);
    assert_eq!(counts.live_transaction_parser_decodes, 0);
    assert_eq!(counts.full_instruction_tree_scans, 1);

    hot_path_metrics::reset();
    let captured = route_update_for_hot_path_harness_with_capture(
        transaction_update(2, FixtureKind::PumpSell),
        true,
    )
    .expect("normalize live fixture with capture");
    assert!(matches!(
        captured,
        GeyserEvent::Transaction {
            mpcf_payload_bytes: Some(_),
            ..
        }
    ));
    let counts = hot_path_metrics::snapshot();
    assert_eq!(counts.live_transaction_prost_encodes, 1);
    assert_eq!(counts.live_transaction_normalizer_decodes, 0);
}

#[test]
fn canonical_parity_snapshot_detects_economic_and_state_drift() {
    let parser = BinaryParser::new(false);
    let event = normalize_transaction(7, FixtureKind::PumpBuy);
    let bundle = parser
        .parse_transaction_bundle(&event)
        .expect("parity fixture bundle");
    let baseline = canonical_parser_parity_snapshot("pump_buy", &bundle);
    let baseline_digest = canonical_parity_digest(std::slice::from_ref(&baseline));

    let mut economic_drift = baseline.clone();
    economic_drift.trades[0].amount = economic_drift.trades[0].amount.saturating_add(1);
    assert_ne!(
        baseline_digest,
        canonical_parity_digest(&[economic_drift]),
        "amount drift must change the canonical parity digest"
    );

    let mut state_drift = baseline.clone();
    state_drift.trades[0].virtual_sol_reserves = Some(123_456);
    assert_ne!(
        baseline_digest,
        canonical_parity_digest(&[state_drift]),
        "reserve-state drift must change the canonical parity digest"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr1b_slow_sinks_never_block_ingest_workers() {
    hot_path_metrics::reset();
    let wal_dir = tempdir().expect("temp WAL dir");
    let wal = Arc::new(Wal::new(wal_dir.path(), 60_000, 120_000).expect("test WAL"));
    let (ipc_sender, _ipc_receiver, _) = create_ipc_channel(IpcChannelConfig::default());
    let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender).with_wal(Arc::clone(&wal));
    hot_path_metrics::set_synthetic_wal_delay(Duration::from_millis(100));

    let started = Instant::now();
    seer.process_event(normalize_transaction(92, FixtureKind::PumpBuy))
        .await
        .expect("event worker must enqueue WAL work");
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "100ms physical WAL delay must not execute on the event worker"
    );
    sleep(Duration::from_millis(120)).await;
    hot_path_metrics::set_synthetic_wal_delay(Duration::ZERO);

    let config = IpcChannelConfig {
        buffer_size: 1,
        backpressure_policy: BackpressurePolicy::Block,
        ..IpcChannelConfig::default()
    };
    let (sender, _slow_receiver, _) = create_ipc_channel(config);
    let parser = BinaryParser::new(false);
    let trade = parser
        .parse_transaction_bundle(&normalize_transaction(93, FixtureKind::PumpBuy))
        .expect("trade fixture")
        .trades
        .into_iter()
        .next()
        .expect("one trade");
    let mut saw_gap = false;
    for _ in 0..32 {
        let send_started = Instant::now();
        let result = sender
            .send_trade(trade.clone(), EventPriority::Normal)
            .await;
        assert!(
            send_started.elapsed() < Duration::from_millis(50),
            "IPC egress saturation must return without awaiting downstream capacity"
        );
        if matches!(result, Err(IpcError::LocalProcessingGap)) {
            saw_gap = true;
            break;
        }
    }
    assert!(
        saw_gap,
        "bounded IPC dispatcher must fail closed on saturation"
    );
    assert!(sender.has_unrecovered_local_gap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run explicitly for the PR1B before/after performance receipt"]
async fn pr1b_hot_path_harness() {
    hot_path_metrics::reset();
    let parser = BinaryParser::new(false);
    let fixtures = [
        ("ordinary_pump_buy", FixtureKind::PumpBuy, 1_u8),
        ("ordinary_pump_sell", FixtureKind::PumpSell, 2_u8),
        (
            "create_and_initial_buy",
            FixtureKind::CreateAndInitialBuy,
            3_u8,
        ),
        (
            "multiple_pump_mutations",
            FixtureKind::MultiplePumpMutations,
            4_u8,
        ),
        (
            "pumpswap_trade_with_inner_instructions",
            FixtureKind::PumpSwapInnerTrade,
            5_u8,
        ),
    ];

    let mut normalized = Vec::with_capacity(fixtures.len());
    let mut business = Vec::with_capacity(fixtures.len());
    for (name, kind, seed) in fixtures {
        let event = normalize_transaction(seed, kind);
        match &event {
            GeyserEvent::Transaction {
                provider_id,
                provider_role,
                tx_index,
                mpcf_payload_bytes,
                mpcf_payload_missing_reason,
                ..
            } => {
                assert_eq!(provider_id.as_deref(), Some("pr1b-primary"));
                assert_eq!(*provider_role, Some(RawProviderRoleV1::PrimaryAuthority));
                assert_eq!(*tx_index, Some(0));
                assert!(mpcf_payload_bytes
                    .as_ref()
                    .is_some_and(|raw| !raw.is_empty()));
                assert_eq!(
                    *mpcf_payload_missing_reason,
                    RawBytesMissingReason::NotMissing
                );
            }
            other => panic!("expected normalized transaction, got {other:?}"),
        }
        let bundle = parser
            .parse_transaction_bundle(&event)
            .expect("fixture bundle must parse");
        business.push(canonical_parser_parity_snapshot(name, &bundle));
        normalized.push((kind, seed, event));
    }

    assert_eq!(business[0].trades.len(), 1);
    assert_eq!(business[1].trades.len(), 1);
    assert!(business[2].initialize_pool.is_some());
    assert_eq!(business[2].trades.len(), 1);
    assert_eq!(business[3].trades.len(), 2);
    assert_eq!(business[4].trades.len(), 1);
    assert!(business[4].trades[0]
        .provenance
        .as_ref()
        .is_some_and(|provenance| provenance.from_cpi));

    let operation_counts = hot_path_metrics::snapshot();

    let account_event = minimal_account_event();
    match &account_event {
        GeyserEvent::AccountUpdate {
            provider_id,
            provider_role,
            write_version,
            txn_signature,
            ..
        } => {
            assert_eq!(provider_id.as_deref(), Some("pr1b-primary"));
            assert_eq!(*provider_role, Some(RawProviderRoleV1::PrimaryAuthority));
            assert_eq!(*write_version, Some(0));
            assert_eq!(*txn_signature, Some(deterministic_signature(77)));
        }
        other => panic!("expected account update, got {other:?}"),
    }

    let mut receive_to_normalize_ns = Vec::with_capacity(HARNESS_ITERATIONS * fixtures.len());
    let mut normalize_to_bundle_ns = Vec::with_capacity(HARNESS_ITERATIONS * fixtures.len());
    let throughput_started = Instant::now();
    let mut processed = 0usize;
    for iteration in 0..HARNESS_ITERATIONS {
        for (kind, seed, _) in &normalized {
            let normalize_started = Instant::now();
            let event = normalize_transaction(seed.wrapping_add(iteration as u8), *kind);
            receive_to_normalize_ns.push(normalize_started.elapsed().as_nanos() as u64);

            let parse_started = Instant::now();
            parser
                .parse_transaction_bundle(&event)
                .expect("performance fixture must parse");
            normalize_to_bundle_ns.push(parse_started.elapsed().as_nanos() as u64);
            processed += 1;
        }
    }
    let throughput_elapsed = throughput_started.elapsed();
    let throughput_events_per_second =
        processed as f64 / throughput_elapsed.as_secs_f64().max(f64::EPSILON);

    let (queue_burst, burst_account_event) = queue_burst_measurement();
    let saturation = saturation_measurement(burst_account_event);

    hot_path_metrics::reset();
    let wal_dir = tempdir().expect("temp WAL dir");
    let wal = Arc::new(Wal::new(wal_dir.path(), 60_000, 120_000).expect("test WAL"));
    let (wal_ipc_sender, _wal_ipc_receiver, _) = create_ipc_channel(IpcChannelConfig::default());
    let wal_seer =
        Seer::new_with_ipc(SeerConfig::default(), wal_ipc_sender).with_wal(Arc::clone(&wal));
    hot_path_metrics::set_synthetic_wal_delay(Duration::from_millis(5));
    let slow_wal_started = Instant::now();
    wal_seer
        .process_event(normalize_transaction(90, FixtureKind::PumpBuy))
        .await
        .expect("slow WAL fixture");
    let slow_wal_enqueue_elapsed_ns = slow_wal_started.elapsed().as_nanos() as u64;
    let writer_deadline = Instant::now() + Duration::from_millis(250);
    while hot_path_metrics::snapshot().wal_append_calls < 2 && Instant::now() < writer_deadline {
        sleep(Duration::from_millis(1)).await;
    }
    let slow_wal_writer_elapsed_ns = slow_wal_started.elapsed().as_nanos() as u64;
    hot_path_metrics::set_synthetic_wal_delay(Duration::ZERO);
    let wal_counts = hot_path_metrics::snapshot();

    hot_path_metrics::reset();
    let ipc_config = IpcChannelConfig {
        buffer_size: 1,
        backpressure_policy: BackpressurePolicy::Block,
        ..IpcChannelConfig::default()
    };
    let (ipc_sender, mut ipc_receiver, _) = create_ipc_channel(ipc_config);
    let ipc_trade = parser
        .parse_transaction_bundle(&normalized[0].2)
        .expect("IPC fixture parse")
        .trades
        .into_iter()
        .next()
        .expect("IPC fixture trade");
    ipc_sender
        .send_trade(ipc_trade.clone(), EventPriority::Normal)
        .await
        .expect("seed IPC queue");
    let dispatcher_deadline = Instant::now() + Duration::from_millis(100);
    while ipc_sender.dispatcher_queue_len() != 0 && Instant::now() < dispatcher_deadline {
        sleep(Duration::from_millis(1)).await;
    }
    assert_eq!(
        ipc_sender.dispatcher_queue_len(),
        0,
        "seed event must reach the deliberately undrained downstream queue"
    );
    let slow_ipc_started = Instant::now();
    ipc_sender
        .send_trade(ipc_trade, EventPriority::Normal)
        .await
        .expect("nonblocking IPC egress enqueue");
    let slow_ipc_enqueue_elapsed_ns = slow_ipc_started.elapsed().as_nanos() as u64;
    sleep(Duration::from_millis(10)).await;
    ipc_receiver.recv().await.expect("drain seeded IPC event");
    let ipc_counts = hot_path_metrics::snapshot();

    let business_digest = canonical_parity_digest(&business);
    assert_eq!(
        business_digest, BASELINE_CANONICAL_PARITY_DIGEST,
        "PR1B must preserve the full frozen-corpus canonical parser snapshot"
    );
    let fixture_summary = business
        .iter()
        .map(|snapshot| {
            json!({
                "fixture": snapshot.fixture,
                "initialize_pool": snapshot.initialize_pool.is_some(),
                "trade_count": snapshot.trades.len(),
            })
        })
        .collect::<Vec<_>>();

    let report = json!({
        "schema": "ghost_pr1b_ingest_hot_path_harness_v1",
        "workload": {
            "iterations": HARNESS_ITERATIONS,
            "transaction_events": processed,
            "fixtures": fixture_summary,
            "account_update": true,
            "burst_input": BURST_EVENTS,
            "slow_wal_sink": true,
            "slow_ipc_consumer": true,
            "queue_saturation": true,
        },
        "canonical_parser_parity_digest_blake3": business_digest,
        "throughput_events_per_second": throughput_events_per_second,
        "receive_to_normalize": latency_summary(&receive_to_normalize_ns),
        "normalize_to_parsed_bundle": latency_summary(&normalize_to_bundle_ns),
        "queue": queue_burst,
        "saturation": saturation,
        "steady_state_rss_kib": steady_state_rss_kib(),
        "cpu_time": Value::Null,
        "operation_counts_for_five_transactions": {
            "prost_encode": operation_counts.live_transaction_prost_encodes,
            "normalizer_prost_decode": operation_counts.live_transaction_normalizer_decodes,
            "parser_prost_decode": operation_counts.live_transaction_parser_decodes,
            "full_instruction_tree_scans": operation_counts.full_instruction_tree_scans,
        },
        "slow_wal": {
            "enqueue_elapsed_ns": slow_wal_enqueue_elapsed_ns,
            "writer_elapsed_ns": slow_wal_writer_elapsed_ns,
            "append_calls": wal_counts.wal_append_calls,
            "blocking_waits": wal_counts.wal_blocking_waits,
        },
        "slow_ipc": {
            "enqueue_elapsed_ns": slow_ipc_enqueue_elapsed_ns,
            "blocked_before_consume": false,
            "blocking_waits": ipc_counts.ipc_blocking_waits,
        },
        "source": GRPC_GLOBAL_STREAM_SOURCE_LABEL,
    });

    println!(
        "PR1B_HOT_PATH_REPORT={}",
        serde_json::to_string_pretty(&report).expect("serialize report")
    );
}
