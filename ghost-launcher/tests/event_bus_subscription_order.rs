//! Test: Event Bus Subscription Order - Gatekeeper Fix Validation
//!
//! This test validates that event subscribers receive events emitted AFTER subscription.
//! It specifically tests the fix for the Gatekeeper issue where Oracle Runtime was
//! subscribing AFTER Seer started emitting NewPoolDetected events.
//!
//! Scenario:
//! 1. Create event bus
//! 2. Subscribe receiver BEFORE emitting events
//! 3. Emit events
//! 4. Verify subscriber receives all events
//!
//! Anti-pattern (what was happening before the fix):
//! 1. Create event bus
//! 2. Emit events
//! 3. Subscribe receiver AFTER events were emitted
//! 4. Subscriber misses early events (tokio::sync::broadcast doesn't buffer)

use ghost_launcher::events::{create_event_bus, DetectedPool, GhostEvent};
use solana_sdk::pubkey::Pubkey;
use std::time::Duration;
use tokio::time::{sleep, timeout};

#[tokio::test]
async fn test_subscription_before_emission_receives_all_events() {
    // Create event bus
    let (event_tx, _event_rx) = create_event_bus();

    // CRITICAL: Subscribe BEFORE emitting events (correct order)
    let mut subscriber = event_tx.subscribe();

    // Emit 3 NewPoolDetected events
    for i in 0..3 {
        let pool_pubkey = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();

        let detected_pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey.to_string(),
            base_mint: base_mint.to_string(),
            quote_mint: quote_mint.to_string(),
            amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            bonding_curve: Pubkey::new_unique().to_string(),
            creator: creator.to_string(),
            slot: Some(12345 + i),
            tx_index: None,
            timestamp_ms: 1700000000000 + i as u64,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1700000000123 + i as u64),
            initial_liquidity_sol: Some(10.0),
            signature: format!("test_sig_{}", i),
        };

        event_tx
            .send(GhostEvent::new_pool_detected(detected_pool))
            .expect("Failed to send NewPoolDetected");
    }

    // Verify subscriber received all 3 events
    let mut received_count = 0;
    for _ in 0..3 {
        match timeout(Duration::from_millis(100), subscriber.recv()).await {
            Ok(Ok(GhostEvent::NewPoolDetected(_))) => {
                received_count += 1;
            }
            Ok(Ok(other)) => {
                panic!("Unexpected event type: {:?}", other);
            }
            Ok(Err(e)) => {
                panic!("Error receiving event: {}", e);
            }
            Err(_) => {
                break; // Timeout - no more events
            }
        }
    }

    assert_eq!(
        received_count, 3,
        "Expected to receive 3 events, but got {}",
        received_count
    );
    println!("✅ PASS: Subscriber received all 3 events when subscribing BEFORE emission");
}

#[tokio::test]
async fn test_subscription_after_emission_misses_events() {
    // Create event bus
    let (event_tx, _event_rx) = create_event_bus();

    // Emit 3 NewPoolDetected events BEFORE subscribing (anti-pattern - what was wrong before)
    for i in 0..3 {
        let pool_pubkey = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();

        let detected_pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey.to_string(),
            base_mint: base_mint.to_string(),
            quote_mint: quote_mint.to_string(),
            amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            bonding_curve: Pubkey::new_unique().to_string(),
            creator: creator.to_string(),
            slot: Some(12345 + i),
            tx_index: None,
            timestamp_ms: 1700000000000 + i as u64,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1700000000123 + i as u64),
            initial_liquidity_sol: Some(10.0),
            signature: format!("test_sig_{}", i),
        };

        event_tx
            .send(GhostEvent::new_pool_detected(detected_pool))
            .expect("Failed to send NewPoolDetected");
    }

    // Wait a bit to ensure events are "gone"
    sleep(Duration::from_millis(50)).await;

    // WRONG: Subscribe AFTER events were emitted
    let mut subscriber = event_tx.subscribe();

    // Try to receive events - should get none because we subscribed late
    match timeout(Duration::from_millis(200), subscriber.recv()).await {
        Ok(Ok(_)) => {
            panic!(
                "Unexpected: Received event after late subscription (broadcast shouldn't buffer)"
            );
        }
        Ok(Err(_)) => {
            // Channel error (e.g., closed) - acceptable for this test
        }
        Err(_) => {
            // Timeout - expected! No events because we subscribed after they were emitted
            println!("✅ PASS: Late subscriber received no events (as expected - broadcast doesn't buffer)");
        }
    }
}

#[tokio::test]
async fn test_multiple_subscribers_before_emission() {
    // Create event bus
    let (event_tx, _event_rx) = create_event_bus();

    // Subscribe 2 subscribers BEFORE emitting events
    let mut subscriber1 = event_tx.subscribe();
    let mut subscriber2 = event_tx.subscribe();

    // Emit 1 event
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let creator = Pubkey::new_unique();

    let detected_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: quote_mint.to_string(),
        amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: creator.to_string(),
        slot: Some(12345),
        tx_index: None,
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(1700000000123),
        initial_liquidity_sol: Some(10.0),
        signature: "test_sig_1".to_string(),
    };

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool))
        .expect("Failed to send NewPoolDetected");

    // Both subscribers should receive the event
    match timeout(Duration::from_millis(100), subscriber1.recv()).await {
        Ok(Ok(GhostEvent::NewPoolDetected(_))) => {
            println!("✅ Subscriber 1 received event");
        }
        _ => panic!("Subscriber 1 failed to receive event"),
    }

    match timeout(Duration::from_millis(100), subscriber2.recv()).await {
        Ok(Ok(GhostEvent::NewPoolDetected(_))) => {
            println!("✅ Subscriber 2 received event");
        }
        _ => panic!("Subscriber 2 failed to receive event"),
    }

    println!("✅ PASS: Both subscribers received event when subscribing BEFORE emission");
}
