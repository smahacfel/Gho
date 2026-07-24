use std::{
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use ghost_core::{wal::Wal, RawProviderRoleV1};
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
        route_update_for_hot_path_harness, route_update_for_hot_path_harness_with_capture,
        DualLaneChannel, PumpEvent, TransportStats, GRPC_GLOBAL_STREAM_SOURCE_LABEL,
        PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID,
    },
    hot_path_metrics,
    ipc::{create_ipc_channel, BackpressurePolicy, EventPriority, IpcChannelConfig},
    types::{GeyserEvent, RawBytesMissingReason},
    Seer,
};

const HARNESS_ITERATIONS: usize = 200;
const BURST_EVENTS: usize = 2_048;
const BASELINE_BUSINESS_DIGEST: &str =
    "062d36ab094fb470909fd9836318fee85d89dbed8f1a9a86080041f20a399ee2";

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

fn business_summary(name: &str, bundle: &ParsedTransactionBundle) -> Value {
    json!({
        "fixture": name,
        "initialize_pool": bundle.initialize_pool.as_ref().map(|pool| json!({
            "signature": pool.signature.to_string(),
            "slot": pool.slot,
            "pool": pool.pool_amm_id.to_string(),
            "mint": pool.base_mint.to_string(),
            "provider_id": pool.provider_id,
            "provider_role": pool.provider_role.map(|role| role.as_str()),
        })),
        "trades": bundle.trades.iter().map(|trade| json!({
            "signature": trade.signature.to_string(),
            "slot": trade.slot,
            "pool": trade.pool_amm_id.to_string(),
            "mint": trade.mint.to_string(),
            "is_buy": trade.is_buy,
            "event_ordinal": trade.event_ordinal,
            "provider_id": trade.provider_id,
            "provider_role": trade.provider_role.map(|role| role.as_str()),
            "from_cpi": trade.provenance.as_ref().is_some_and(|provenance| provenance.from_cpi),
        })).collect::<Vec<_>>(),
    })
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

fn queue_burst_measurement() -> (Value, GeyserEvent) {
    let (channel, receiver) = DualLaneChannel::new();
    let stats = Arc::new(TransportStats::default());
    let account_event = minimal_account_event();
    let mut high_water = 0usize;

    for index in 0..BURST_EVENTS {
        channel.send(
            queued_account_event(&account_event, 30_000 + index as u64),
            &stats,
        );
        high_water = high_water.max(channel.depth());
    }

    let oldest_age_ns = receiver
        .fast
        .try_iter()
        .chain(receiver.overflow.try_iter())
        .map(|event| event.received_at().elapsed().as_nanos() as u64)
        .max()
        .unwrap_or_default();

    (
        json!({
            "input_events": BURST_EVENTS,
            "queue_high_water": high_water,
            "oldest_event_age_ns": oldest_age_ns,
            "spilled_events": stats.msgs_spilled.load(std::sync::atomic::Ordering::Relaxed),
        }),
        account_event,
    )
}

fn saturation_measurement(account_event: GeyserEvent) -> Value {
    let (channel, receiver) = DualLaneChannel::with_capacities(1, 1);
    let stats = Arc::new(TransportStats::default());

    assert!(channel.send(queued_account_event(&account_event, 1), &stats));
    assert!(!channel.send(queued_account_event(&account_event, 2), &stats));
    let channel_for_blocked_send = channel.clone();
    let stats_for_blocked_send = Arc::clone(&stats);
    let blocked_event = queued_account_event(&account_event, 3);
    let blocked_started = Instant::now();
    let blocked = std::thread::spawn(move || {
        channel_for_blocked_send.send(blocked_event, &stats_for_blocked_send)
    });
    std::thread::sleep(Duration::from_millis(10));
    let blocked_before_drain = !blocked.is_finished();
    let _ = receiver.overflow.recv().expect("overflow event");
    let _ = blocked.join().expect("blocked sender thread");

    json!({
        "queue_capacity": 2,
        "blocked_before_drain": blocked_before_drain,
        "blocking_wait_ns": blocked_started.elapsed().as_nanos() as u64,
        "silent_drop_count": stats.msgs_overflow_dropped.load(std::sync::atomic::Ordering::Relaxed),
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
        business.push(business_summary(name, &bundle));
        normalized.push((kind, seed, event));
    }

    assert_eq!(business[0]["trades"].as_array().unwrap().len(), 1);
    assert_eq!(business[1]["trades"].as_array().unwrap().len(), 1);
    assert!(!business[2]["initialize_pool"].is_null());
    assert_eq!(business[2]["trades"].as_array().unwrap().len(), 1);
    assert_eq!(business[3]["trades"].as_array().unwrap().len(), 2);
    assert_eq!(business[4]["trades"].as_array().unwrap().len(), 1);
    assert_eq!(business[4]["trades"][0]["from_cpi"], true);

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
    let slow_wal_elapsed_ns = slow_wal_started.elapsed().as_nanos() as u64;
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
    let blocked_sender = ipc_sender.clone();
    let slow_ipc_started = Instant::now();
    let blocked_send = tokio::spawn(async move {
        blocked_sender
            .send_trade(ipc_trade, EventPriority::Normal)
            .await
    });
    sleep(Duration::from_millis(10)).await;
    let blocked_before_consume = !blocked_send.is_finished();
    ipc_receiver.recv().await.expect("drain seeded IPC event");
    blocked_send
        .await
        .expect("IPC sender task")
        .expect("IPC send after drain");
    let slow_ipc_elapsed_ns = slow_ipc_started.elapsed().as_nanos() as u64;
    let ipc_counts = hot_path_metrics::snapshot();

    let business_digest = blake3::hash(
        serde_json::to_vec(&business)
            .expect("business summary JSON")
            .as_slice(),
    )
    .to_hex()
    .to_string();
    assert_eq!(
        business_digest, BASELINE_BUSINESS_DIGEST,
        "PR1B must preserve frozen-corpus business semantics"
    );

    let report = json!({
        "schema": "ghost_pr1b_ingest_hot_path_harness_v1",
        "workload": {
            "iterations": HARNESS_ITERATIONS,
            "transaction_events": processed,
            "fixtures": business,
            "account_update": true,
            "burst_input": BURST_EVENTS,
            "slow_wal_sink": true,
            "slow_ipc_consumer": true,
            "queue_saturation": true,
        },
        "business_digest_blake3": business_digest,
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
            "elapsed_ns": slow_wal_elapsed_ns,
            "append_calls": wal_counts.wal_append_calls,
            "blocking_waits": wal_counts.wal_blocking_waits,
        },
        "slow_ipc": {
            "elapsed_ns": slow_ipc_elapsed_ns,
            "blocked_before_consume": blocked_before_consume,
            "blocking_waits": ipc_counts.ipc_blocking_waits,
        },
        "source": GRPC_GLOBAL_STREAM_SOURCE_LABEL,
    });

    println!(
        "PR1B_HOT_PATH_REPORT={}",
        serde_json::to_string_pretty(&report).expect("serialize report")
    );
}
