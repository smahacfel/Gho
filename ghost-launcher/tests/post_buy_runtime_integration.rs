//! Integration Test: PostBuyRuntime event lifecycle via ghost-brain
//!
//! Validates the complete post-buy lifecycle in PAPER mode using
//! ghost-brain's real PaperBackend, AemRuntime, and EventEmitter:
//! 1. Boots event bus + PostBuyRuntime
//! 2. Injects a synthetic PostBuySubmitted event
//! 3. Asserts ghost-brain's JSONL events contain:
//!    Candidate, EntrySubmitted, EntryFilled, PositionOpened, AemTick,
//!    ManagementDecision, ExitSubmitted, ExitFilled, PositionClosed
//!
//! No network calls.

use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_launcher::components::post_buy_runtime::PostBuyRuntimeConfig;
use ghost_launcher::events::{create_event_bus, GhostEvent, PostBuySource};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

/// Read all JSONL files in a directory and extract event types from ghost-brain format.
/// Ghost-brain serializes EventKind as: {"type": "<EventType>", "payload": {...}}
/// Full event: {"envelope": {...}, "kind": {"type": "<EventType>", "payload": {...}}}
fn read_events_from_dir(dir: &std::path::Path) -> Vec<serde_json::Value> {
    let mut events = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "jsonl") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for line in content.lines() {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                            events.push(v);
                        }
                    }
                }
            }
        }
    }
    events
}

fn read_event_types_from_dir(dir: &std::path::Path) -> Vec<String> {
    read_events_from_dir(dir)
        .into_iter()
        .filter_map(|v| {
            v.get("kind")
                .and_then(|k| k.get("type"))
                .and_then(|t| t.as_str())
                .map(str::to_string)
        })
        .collect()
}

#[tokio::test]
async fn test_post_buy_runtime_paper_lifecycle() {
    // Setup temp dir for events
    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let events_dir = tmp_dir.path().join("events");
    std::fs::create_dir_all(&events_dir).expect("failed to create events dir");

    // Create event bus
    let (event_tx, _event_rx) = create_event_bus();

    // Create shutdown channel
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // Subscribe PostBuyRuntime BEFORE any events are sent (critical ordering)
    let post_buy_rx = event_tx.subscribe();
    let post_buy_shutdown_rx = shutdown_tx.subscribe();

    let config = PostBuyRuntimeConfig {
        events_output_path: events_dir.clone(),
        paper_fill_delay_min_ms: 50, // Short delays for test
        paper_fill_delay_max_ms: 100,
        tick_interval_ms: 50,     // Fast ticks for test
        max_ticks_before_exit: 5, // Quick exit for test
        execution_mode: "paper".to_string(),
        aem_t_s: 1, // Short horizon for deterministic ManagementDecision
        max_concurrent_positions: 1,
        position_limit_tracker: None,
        live_sell: None,
        live_position_registry: None,
        slippage_tolerance: 0.20,
        live_exit_take_profit_pct: 0.02,
        live_exit_stop_loss_pct: 0.02,
        shadow_lifecycle_log_path: None,
        account_state_core: Some(Arc::new(AccountStateReducer::new())),
        shadow_ledger: Some(Arc::new(ShadowLedger::new())),
    };

    // Spawn PostBuyRuntime
    let runtime_handle = tokio::spawn(async move {
        ghost_launcher::components::post_buy_runtime::run(
            post_buy_rx,
            post_buy_shutdown_rx,
            None,
            config,
        )
        .await;
    });

    // Give runtime a moment to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Inject synthetic PostBuySubmitted event
    event_tx
        .send(GhostEvent::post_buy_submitted(
            "TestPool123",
            "TestMint456",
            "TestSig789",
            0.5,   // 0.5 SOL
            10000, // tip
            "paper",
            1,
            None,
            PostBuySource::LiveBuy,
            None,
            None,
            None,
            None,
        ))
        .expect("failed to send PostBuySubmitted");

    // Wait for lifecycle to complete (fill delay + ticks + exit)
    // 100ms fill + 5 ticks * 50ms + 500ms exit polling = ~1s, give 5s margin
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Shutdown
    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(2), runtime_handle).await;

    // Read ghost-brain JSONL events from output dir
    let event_types = read_event_types_from_dir(&events_dir);
    let events = read_events_from_dir(&events_dir);
    let expected_candidate_id = "TestMint456_TestPool123_TestSig789";

    assert!(
        !event_types.is_empty(),
        "events directory should contain JSONL files with events"
    );

    // Verify required events are present
    assert!(
        event_types.contains(&"Candidate".to_string()),
        "Missing Candidate event. Found: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"EntrySubmitted".to_string()),
        "Missing EntrySubmitted event. Found: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"EntryFilled".to_string()),
        "Missing EntryFilled event. Found: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"PositionOpened".to_string()),
        "Missing PositionOpened event. Found: {:?}",
        event_types
    );

    // Must have at least 3 AemTicks (ghost-brain AemRuntime produces these)
    let aem_tick_count = event_types.iter().filter(|t| *t == "AemTick").count();
    assert!(
        aem_tick_count >= 3,
        "Expected >= 3 AemTick events from ghost-brain AemRuntime, got {}. Found: {:?}",
        aem_tick_count,
        event_types
    );

    // Must have at least 1 ManagementDecision from ghost-brain AEM
    assert!(
        event_types.contains(&"ManagementDecision".to_string()),
        "Missing ManagementDecision event from ghost-brain AEM. Found: {:?}",
        event_types
    );

    // Must have ExitSubmitted and ExitFilled
    assert!(
        event_types.contains(&"ExitSubmitted".to_string()),
        "Missing ExitSubmitted event. Found: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"ExitFilled".to_string()),
        "Missing ExitFilled event. Found: {:?}",
        event_types
    );

    // Must have PositionClosed
    assert!(
        event_types.contains(&"PositionClosed".to_string()),
        "Missing PositionClosed event. Found: {:?}",
        event_types
    );

    // Verify correct ordering: Candidate < EntrySubmitted < EntryFilled < PositionOpened < AemTick
    let candidate_idx = event_types.iter().position(|t| t == "Candidate").unwrap();
    let entry_sub_idx = event_types
        .iter()
        .position(|t| t == "EntrySubmitted")
        .unwrap();
    let entry_fill_idx = event_types.iter().position(|t| t == "EntryFilled").unwrap();
    let pos_opened_idx = event_types
        .iter()
        .position(|t| t == "PositionOpened")
        .unwrap();
    let aem_tick_idx = event_types.iter().position(|t| t == "AemTick").unwrap();

    assert!(
        candidate_idx < entry_sub_idx,
        "Candidate should come before EntrySubmitted"
    );
    assert!(
        entry_sub_idx < entry_fill_idx,
        "EntrySubmitted should come before EntryFilled"
    );
    assert!(
        entry_fill_idx < pos_opened_idx,
        "EntryFilled should come before PositionOpened"
    );
    assert!(
        pos_opened_idx < aem_tick_idx,
        "PositionOpened should come before AemTick"
    );

    let candidate_ids: std::collections::HashSet<String> = events
        .iter()
        .filter_map(|event| {
            event
                .get("envelope")
                .and_then(|env| env.get("candidate_id"))
                .and_then(|id| id.as_str())
                .map(str::to_string)
        })
        .collect();
    assert_eq!(
        candidate_ids,
        std::collections::HashSet::from([expected_candidate_id.to_string()])
    );

    let position_closed = events
        .iter()
        .find(|event| {
            event
                .get("kind")
                .and_then(|kind| kind.get("type"))
                .and_then(|ty| ty.as_str())
                == Some("PositionClosed")
        })
        .expect("PositionClosed event missing");
    let payload = position_closed
        .get("kind")
        .and_then(|kind| kind.get("payload"))
        .expect("PositionClosed payload missing");
    for field in [
        "entry_value_sol",
        "exit_value_sol",
        "gross_pnl_sol",
        "net_pnl_sol",
        "estimated_costs_sol",
    ] {
        assert!(
            payload
                .get(field)
                .and_then(|value| value.as_f64())
                .is_some(),
            "PositionClosed missing accounting field {field}: {payload:?}"
        );
    }
}

#[tokio::test]
async fn test_post_buy_runtime_no_event_loss_with_early_subscribe() {
    // Verifies that subscribing before sending prevents event loss
    let (event_tx, _event_rx) = create_event_bus();
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let events_dir = tmp_dir.path().join("events");
    std::fs::create_dir_all(&events_dir).expect("failed to create events dir");

    // Subscribe BEFORE sending
    let post_buy_rx = event_tx.subscribe();
    let post_buy_shutdown_rx = shutdown_tx.subscribe();

    let config = PostBuyRuntimeConfig {
        events_output_path: events_dir.clone(),
        paper_fill_delay_min_ms: 50,
        paper_fill_delay_max_ms: 80,
        tick_interval_ms: 50,
        max_ticks_before_exit: 3,
        execution_mode: "paper".to_string(),
        aem_t_s: 1, // Short horizon for deterministic ManagementDecision
        max_concurrent_positions: 2,
        position_limit_tracker: None,
        live_sell: None,
        live_position_registry: None,
        slippage_tolerance: 0.20,
        live_exit_take_profit_pct: 0.02,
        live_exit_stop_loss_pct: 0.02,
        shadow_lifecycle_log_path: None,
        account_state_core: Some(Arc::new(AccountStateReducer::new())),
        shadow_ledger: Some(Arc::new(ShadowLedger::new())),
    };

    let runtime_handle = tokio::spawn(async move {
        ghost_launcher::components::post_buy_runtime::run(
            post_buy_rx,
            post_buy_shutdown_rx,
            None,
            config,
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send two events rapidly
    for i in 0..2 {
        event_tx
            .send(GhostEvent::post_buy_submitted(
                format!("TestPool{}", i),
                format!("TestMint{}", i),
                format!("TestSig{}", i),
                0.1 * (i as f64 + 1.0),
                5000,
                "paper",
                i as u64,
                None,
                PostBuySource::LiveBuy,
                None,
                None,
                None,
                None,
            ))
            .expect("send should succeed");
    }

    // Wait for both lifecycles
    tokio::time::sleep(Duration::from_secs(5)).await;

    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(2), runtime_handle).await;

    let event_types = read_event_types_from_dir(&events_dir);

    // Should have Candidate events for both positions
    let candidate_count = event_types.iter().filter(|t| *t == "Candidate").count();
    assert_eq!(
        candidate_count, 2,
        "Should have 2 Candidate events (one per position), got {}. Events: {:?}",
        candidate_count, event_types
    );
}

/// Verifies that a `lane="live"` PostBuySubmitted routes to the Sender-owned live-sell path
/// and does NOT invoke `PaperPositionLifecycle`.
///
/// The live path is confirmed by:
/// 1. The bulkhead slot remaining reserved after entry metadata resolution fails
///    (no real RPC — deliberately unreachable endpoint), so additional BUYs stay blocked.
/// 2. No paper lifecycle JSONL events written (paper lifecycle writes Candidate, EntryFilled, etc.)
#[tokio::test]
async fn test_live_lane_routes_to_sender_not_paper_lifecycle() {
    use ghost_core::account_state_core::reducer::AccountStateReducer;
    use ghost_core::shadow_ledger::ShadowLedger;
    use ghost_launcher::components::post_buy_runtime::LiveSellHandle;
    use ghost_launcher::components::trigger::safety::PositionLimitTracker;
    use solana_sdk::signature::Keypair;
    use std::sync::Arc;

    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let events_dir = tmp_dir.path().join("events");
    std::fs::create_dir_all(&events_dir).expect("failed to create events dir");

    let (event_tx, event_rx) = create_event_bus();
    let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

    // Tracker with 1 slot: we acquire it manually to simulate the BUY side having reserved it.
    let tracker = PositionLimitTracker::new(1);
    let owner = solana_sdk::pubkey::Pubkey::new_unique();
    let tracked_mint = solana_sdk::pubkey::Pubkey::new_unique();
    let lease = tracker
        .try_acquire(&owner, &tracked_mint, "test_pool")
        .expect("acquire slot");
    let slot_id = lease.slot_id;
    lease.retain(); // live lifecycle now owns the slot

    // LiveSellHandle with deliberately unreachable RPC endpoint.
    // Entry metadata resolution will fail fast, causing the live lifecycle to abort
    // before SELL and keep the slot reserved fail-closed.
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        "http://127.0.0.1:1".to_string(), // no validator on port 1
    ));
    let live_sell = LiveSellHandle {
        rpc_client: Arc::clone(&rpc_client),
        live_tx_sender: Arc::new(
            ghost_launcher::components::live_tx_sender::LiveTxSender::new(
                ghost_launcher::components::live_tx_sender::LiveTxSenderConfig::new(
                    "test://sender-success",
                    "http://127.0.0.1:18081",
                    "test://yellowstone-confirmed",
                    "test-yellowstone-token",
                ),
            )
            .expect("test live tx sender"),
        ),
        payer: Arc::new(Keypair::new()),
        account_state_core: Arc::new(AccountStateReducer::new()),
        shadow_ledger: Arc::new(ShadowLedger::new()),
    };

    let config = PostBuyRuntimeConfig {
        events_output_path: events_dir.clone(),
        paper_fill_delay_min_ms: 10,
        paper_fill_delay_max_ms: 20,
        tick_interval_ms: 10,
        max_ticks_before_exit: 2,
        execution_mode: "live".to_string(),
        aem_t_s: 1,
        max_concurrent_positions: 1,
        position_limit_tracker: Some(tracker.clone()),
        live_sell: Some(live_sell),
        live_position_registry: None,
        slippage_tolerance: 0.20,
        live_exit_take_profit_pct: 0.02,
        live_exit_stop_loss_pct: 0.02,
        shadow_lifecycle_log_path: None,
        account_state_core: Some(Arc::new(AccountStateReducer::new())),
        shadow_ledger: Some(Arc::new(ShadowLedger::new())),
    };

    let runtime_handle = tokio::spawn(ghost_launcher::components::post_buy_runtime::run(
        event_rx,
        shutdown_rx,
        None,
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mint = solana_sdk::pubkey::Pubkey::new_unique();
    let pool = solana_sdk::pubkey::Pubkey::new_unique();

    event_tx
        .send(GhostEvent::post_buy_submitted(
            pool.to_string(),
            mint.to_string(),
            "live_tx_sig",
            0.1,
            5000,
            "live",
            1,
            Some(slot_id),
            PostBuySource::LiveBuy,
            None, // min_tokens_out unused for live lane routing (AccountStateCore path)
            None,
            None,
            None,
        ))
        .expect("send PostBuySubmitted");

    // Wait for the live lifecycle to run:
    // query_actual_ata_balance fails (bad RPC, port 1) → retain slot → return.
    // This is fast (<1s) even with timeout handling in the RPC client.
    tokio::time::sleep(Duration::from_secs(5)).await;

    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(3), runtime_handle).await;

    // KEY ASSERTION 1: bulkhead slot stays reserved because the live position could still be open.
    // This is the critical safety behavior that blocks the next BUY after a failed live exit.
    assert_eq!(
        tracker.active_positions(),
        1,
        "live lifecycle failure must keep the bulkhead slot reserved"
    );
    let second_mint = solana_sdk::pubkey::Pubkey::new_unique();
    assert!(
        tracker
            .try_acquire(&owner, &second_mint, "second_pool")
            .is_err(),
        "max_concurrent_positions must still block a second live BUY after exit failure"
    );

    // KEY ASSERTION 2: no paper-lifecycle JSONL events (Candidate, EntryFilled, etc.)
    // PaperPositionLifecycle writes these events to the EventEmitter → JSONL files.
    // The live path bypasses PaperPositionLifecycle entirely; no such events should appear.
    let paper_event_types = read_event_types_from_dir(&events_dir);
    let paper_lifecycle_events: Vec<_> = paper_event_types
        .iter()
        .filter(|t| {
            matches!(
                t.as_str(),
                "Candidate" | "EntryFilled" | "PositionOpened" | "AemTick" | "ExitFilled"
            )
        })
        .collect();
    assert!(
        paper_lifecycle_events.is_empty(),
        "live lane must NOT invoke PaperPositionLifecycle — found paper events: {:?}",
        paper_lifecycle_events
    );
}

#[tokio::test]
async fn test_live_lane_without_handle_fails_closed_instead_of_paper_fallback() {
    use ghost_launcher::components::trigger::safety::PositionLimitTracker;

    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let events_dir = tmp_dir.path().join("events");
    std::fs::create_dir_all(&events_dir).expect("failed to create events dir");

    let (event_tx, event_rx) = create_event_bus();
    let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

    let tracker = PositionLimitTracker::new(1);
    let owner = solana_sdk::pubkey::Pubkey::new_unique();
    let tracked_mint = solana_sdk::pubkey::Pubkey::new_unique();
    let lease = tracker
        .try_acquire(&owner, &tracked_mint, "test_pool")
        .expect("acquire slot");
    let slot_id = lease.slot_id;
    lease.retain();

    let config = PostBuyRuntimeConfig {
        events_output_path: events_dir.clone(),
        paper_fill_delay_min_ms: 10,
        paper_fill_delay_max_ms: 20,
        tick_interval_ms: 10,
        max_ticks_before_exit: 2,
        execution_mode: "live".to_string(),
        aem_t_s: 1,
        max_concurrent_positions: 1,
        position_limit_tracker: Some(tracker.clone()),
        live_sell: None,
        live_position_registry: None,
        slippage_tolerance: 0.20,
        live_exit_take_profit_pct: 0.02,
        live_exit_stop_loss_pct: 0.02,
        shadow_lifecycle_log_path: None,
        account_state_core: Some(Arc::new(AccountStateReducer::new())),
        shadow_ledger: Some(Arc::new(ShadowLedger::new())),
    };

    let runtime_handle = tokio::spawn(ghost_launcher::components::post_buy_runtime::run(
        event_rx,
        shutdown_rx,
        None,
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mint = solana_sdk::pubkey::Pubkey::new_unique();
    let pool = solana_sdk::pubkey::Pubkey::new_unique();

    event_tx
        .send(GhostEvent::post_buy_submitted(
            pool.to_string(),
            mint.to_string(),
            "live_tx_sig_no_handle",
            0.1,
            5000,
            "live",
            1,
            Some(slot_id),
            PostBuySource::LiveBuy,
            None,
            None,
            None,
            None,
        ))
        .expect("send PostBuySubmitted");

    tokio::time::sleep(Duration::from_millis(250)).await;

    let _ = shutdown_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(3), runtime_handle).await;

    assert_eq!(
        tracker.active_positions(),
        1,
        "live lane without handle must keep the bulkhead slot reserved fail-closed"
    );
    let second_mint = solana_sdk::pubkey::Pubkey::new_unique();
    assert!(
        tracker
            .try_acquire(&owner, &second_mint, "second_pool")
            .is_err(),
        "missing live handle must not free capacity for another BUY"
    );

    let paper_event_types = read_event_types_from_dir(&events_dir);
    assert!(
        paper_event_types.is_empty(),
        "live lane without handle must not fall back to paper lifecycle events: {:?}",
        paper_event_types
    );
}
