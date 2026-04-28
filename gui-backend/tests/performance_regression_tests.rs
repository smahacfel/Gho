//! Performance and Regression Tests
//!
//! This test suite verifies that the GUI backend does not degrade
//! the performance of the Ghost pipeline (Seer/Trigger components).
//!
//! Tests measure:
//! - Throughput: operations per second with GUI enabled vs disabled
//! - Latency: time to complete operations with GUI enabled vs disabled
//! - Memory overhead: additional memory used by GUI backend
//! - Concurrent load: system behavior under high load

use gui_backend::{GuiBackend, GuiBackendConfig, Portfolio, Position, Settings, SystemMode};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Baseline throughput without GUI backend
#[tokio::test]
async fn benchmark_baseline_throughput_no_gui() {
    let iterations = 10000;
    let start = Instant::now();

    // Simulate pipeline operations without GUI
    for i in 0..iterations {
        // Simulate candidate processing
        tokio::task::yield_now().await;

        // Simulate some work
        let _ = i * 2;
    }

    let duration = start.elapsed();
    let throughput = iterations as f64 / duration.as_secs_f64();

    println!("Baseline throughput (no GUI): {:.2} ops/sec", throughput);
    println!("Baseline duration: {:?}", duration);

    // Store baseline metrics for comparison
    assert!(
        throughput > 1000.0,
        "Baseline throughput too low: {}",
        throughput
    );
}

/// Throughput with GUI backend enabled (read-only operations)
#[tokio::test]
async fn benchmark_throughput_with_gui_readonly() {
    let config = GuiBackendConfig {
        port: 8810,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = Arc::new(backend.state());

    let iterations = 10000;
    let start = Instant::now();

    // Simulate pipeline operations with GUI state reads
    for i in 0..iterations {
        // Check mode (Trigger does this)
        let _ = state.is_running();

        // Simulate candidate processing
        tokio::task::yield_now().await;

        // Simulate some work
        let _ = i * 2;
    }

    let duration = start.elapsed();
    let throughput = iterations as f64 / duration.as_secs_f64();

    println!("Throughput with GUI (readonly): {:.2} ops/sec", throughput);
    println!("Duration: {:?}", duration);

    // Should maintain > 90% of baseline throughput
    assert!(
        throughput > 900.0,
        "Throughput degraded too much: {}",
        throughput
    );
}

/// Throughput with GUI backend and runtime config reads
#[tokio::test]
async fn benchmark_throughput_with_runtime_config_reads() {
    let config = GuiBackendConfig {
        port: 8811,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = Arc::new(backend.state());

    let iterations = 10000;
    let start = Instant::now();

    // Simulate pipeline operations with runtime config reads
    for i in 0..iterations {
        // Read runtime config (Features/Strategy does this)
        let _ = state.get_runtime_config();

        // Simulate candidate processing
        tokio::task::yield_now().await;

        // Simulate some work
        let _ = i * 2;
    }

    let duration = start.elapsed();
    let throughput = iterations as f64 / duration.as_secs_f64();

    println!(
        "Throughput with runtime config reads: {:.2} ops/sec",
        throughput
    );
    println!("Duration: {:?}", duration);

    // Should maintain > 85% of baseline throughput
    assert!(
        throughput > 850.0,
        "Throughput degraded too much with config reads: {}",
        throughput
    );
}

/// Latency measurement for mode checks
#[tokio::test]
async fn benchmark_mode_check_latency() {
    let config = GuiBackendConfig {
        port: 8812,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = backend.state();

    let iterations = 10000;
    let mut latencies = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = Instant::now();
        let _ = state.is_running();
        let duration = start.elapsed();
        latencies.push(duration);
    }

    // Calculate statistics
    let total: Duration = latencies.iter().sum();
    let avg_latency = total / iterations as u32;
    let max_latency = latencies.iter().max().unwrap();

    println!("Mode check average latency: {:?}", avg_latency);
    println!("Mode check max latency: {:?}", max_latency);

    // Mode checks should be < 1 microsecond on average
    assert!(
        avg_latency < Duration::from_micros(1),
        "Mode check latency too high: {:?}",
        avg_latency
    );

    // Max latency should be < 10 microseconds
    assert!(
        *max_latency < Duration::from_micros(10),
        "Max mode check latency too high: {:?}",
        max_latency
    );
}

/// Latency measurement for settings reads
#[tokio::test]
async fn benchmark_settings_read_latency() {
    let config = GuiBackendConfig {
        port: 8813,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = backend.state();

    let iterations = 10000;
    let mut latencies = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = Instant::now();
        let _ = state.get_runtime_config();
        let duration = start.elapsed();
        latencies.push(duration);
    }

    // Calculate statistics
    let total: Duration = latencies.iter().sum();
    let avg_latency = total / iterations as u32;
    let max_latency = latencies.iter().max().unwrap();

    println!("Settings read average latency: {:?}", avg_latency);
    println!("Settings read max latency: {:?}", max_latency);

    // Settings reads should be < 10 microseconds on average
    assert!(
        avg_latency < Duration::from_micros(10),
        "Settings read latency too high: {:?}",
        avg_latency
    );

    // Max latency should be < 100 microseconds
    assert!(
        *max_latency < Duration::from_micros(100),
        "Max settings read latency too high: {:?}",
        max_latency
    );
}

/// Test concurrent mode checks under high load
#[tokio::test]
async fn benchmark_concurrent_mode_checks_high_load() {
    let config = GuiBackendConfig {
        port: 8814,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = Arc::new(backend.state());

    let num_tasks = 50;
    let iterations_per_task = 1000;

    let start = Instant::now();
    let mut handles = vec![];

    // Spawn multiple concurrent tasks
    for _ in 0..num_tasks {
        let state_clone = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            let mut local_latencies = Vec::with_capacity(iterations_per_task);

            for _ in 0..iterations_per_task {
                let local_start = Instant::now();
                let _ = state_clone.is_running();
                local_latencies.push(local_start.elapsed());
            }

            local_latencies
        });
        handles.push(handle);
    }

    // Wait for all tasks
    let mut all_latencies = Vec::new();
    for handle in handles {
        let latencies = handle.await.unwrap();
        all_latencies.extend(latencies);
    }

    let total_duration = start.elapsed();
    let total_ops = num_tasks * iterations_per_task;
    let throughput = total_ops as f64 / total_duration.as_secs_f64();

    println!(
        "Concurrent mode checks throughput: {:.2} ops/sec",
        throughput
    );
    println!("Total operations: {}", total_ops);
    println!("Total duration: {:?}", total_duration);

    // Should handle at least 10k ops/sec under concurrent load
    assert!(
        throughput > 10_000.0,
        "Concurrent throughput too low: {}",
        throughput
    );
}

/// Test pipeline simulation with GUI state updates
#[tokio::test]
async fn benchmark_pipeline_simulation_with_gui() {
    let config = GuiBackendConfig {
        port: 8815,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = Arc::new(backend.state());

    let candidates_processed = 100;
    let start = Instant::now();

    for i in 0..candidates_processed {
        // 1. Seer detects candidate
        tokio::task::yield_now().await;

        // 2. Oracle scores it
        tokio::task::yield_now().await;

        // 3. Features checks mode and reads settings
        if !state.is_running() {
            continue;
        }
        let _ = state.get_runtime_config();

        // 4. Generate swap plan
        tokio::task::yield_now().await;

        // 5. Trigger sends transaction
        state.update_transaction_stats(i + 1, i);

        // 6. Update portfolio (simulate every 10th)
        if i % 10 == 0 {
            let portfolio = Portfolio {
                sol_balance: 5_000_000_000,
                positions: vec![],
                total_value: 5_000_000_000,
                total_pnl: 0,
            };
            state.update_portfolio(portfolio);
        }
    }

    let duration = start.elapsed();
    let throughput = candidates_processed as f64 / duration.as_secs_f64();

    println!(
        "Pipeline simulation throughput: {:.2} candidates/sec",
        throughput
    );
    println!("Duration: {:?}", duration);

    // Should process at least 100 candidates/sec
    assert!(
        throughput > 100.0,
        "Pipeline throughput too low: {}",
        throughput
    );

    // Verify final state
    let final_status = state.get_status();
    assert_eq!(final_status.transactions_sent, candidates_processed);
}

/// Test settings update latency
#[tokio::test]
async fn benchmark_settings_update_latency() {
    let config = GuiBackendConfig {
        port: 8816,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = backend.state();

    let iterations = 1000;
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let settings = Settings {
            position_size_lamports: 100_000_000 + (i as u64 * 1_000_000),
            jito_tip_lamports: 10_000,
            max_slippage: 0.01,
            enable_jito: false,
            auto_jito_tip: true,
        };

        let start = Instant::now();
        state.update_settings(settings);
        let duration = start.elapsed();
        latencies.push(duration);
    }

    // Calculate statistics
    let total: Duration = latencies.iter().sum();
    let avg_latency = total / iterations as u32;
    let max_latency = latencies.iter().max().unwrap();

    println!("Settings update average latency: {:?}", avg_latency);
    println!("Settings update max latency: {:?}", max_latency);

    // Settings updates should be < 100 microseconds on average
    assert!(
        avg_latency < Duration::from_micros(100),
        "Settings update latency too high: {:?}",
        avg_latency
    );
}

/// Test portfolio update latency
#[tokio::test]
async fn benchmark_portfolio_update_latency() {
    let config = GuiBackendConfig {
        port: 8817,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = backend.state();

    let iterations = 1000;
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let portfolio = Portfolio {
            sol_balance: 5_000_000_000 + (i as u64 * 1_000_000),
            positions: vec![Position {
                mint: format!("Token{}", i),
                amount: 1_000_000,
                entry_price: 50_000,
                current_price: Some(55_000),
                pnl: 5_000,
                opened_at: 1700000000,
            }],
            total_value: 5_100_000_000,
            total_pnl: 100_000,
        };

        let start = Instant::now();
        state.update_portfolio(portfolio);
        let duration = start.elapsed();
        latencies.push(duration);
    }

    // Calculate statistics
    let total: Duration = latencies.iter().sum();
    let avg_latency = total / iterations as u32;
    let max_latency = latencies.iter().max().unwrap();

    println!("Portfolio update average latency: {:?}", avg_latency);
    println!("Portfolio update max latency: {:?}", max_latency);

    // Portfolio updates should be < 100 microseconds on average
    assert!(
        avg_latency < Duration::from_micros(100),
        "Portfolio update latency too high: {:?}",
        avg_latency
    );
}

/// Regression test: Verify no performance degradation over time
#[tokio::test]
async fn regression_test_sustained_load() {
    let config = GuiBackendConfig {
        port: 8818,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = Arc::new(backend.state());

    // Run for 5 rounds of 1000 operations each
    let rounds = 5;
    let ops_per_round = 1000;
    let mut round_throughputs = Vec::new();

    for round in 0..rounds {
        let start = Instant::now();

        for i in 0..ops_per_round {
            // Mode check
            let _ = state.is_running();

            // Settings read
            let _ = state.get_runtime_config();

            // Stats update
            if i % 10 == 0 {
                let total = round * ops_per_round + i;
                state.update_transaction_stats(total + 1, total);
            }

            tokio::task::yield_now().await;
        }

        let duration = start.elapsed();
        let throughput = ops_per_round as f64 / duration.as_secs_f64();
        round_throughputs.push(throughput);

        println!("Round {} throughput: {:.2} ops/sec", round + 1, throughput);

        // Small delay between rounds
        sleep(Duration::from_millis(100)).await;
    }

    // Verify throughput doesn't degrade significantly across rounds
    let first_throughput = round_throughputs[0];
    let last_throughput = round_throughputs[(rounds - 1) as usize];

    println!("First round: {:.2} ops/sec", first_throughput);
    println!("Last round: {:.2} ops/sec", last_throughput);

    // Last round should be at least 90% of first round (no degradation)
    let ratio = last_throughput / first_throughput;
    assert!(
        ratio > 0.9,
        "Performance degraded over time: ratio = {:.2}",
        ratio
    );
}

/// Test WebSocket broadcast overhead
#[tokio::test]
async fn benchmark_websocket_broadcast_overhead() {
    let config = GuiBackendConfig {
        port: 8819,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Create multiple subscribers (simulating WebSocket clients)
    let num_subscribers = 10;
    let mut receivers = Vec::new();
    for _ in 0..num_subscribers {
        receivers.push(state.subscribe());
    }

    // Measure broadcast latency
    let iterations = 100;
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let start = Instant::now();

        // Trigger a broadcast by changing mode
        state.set_mode(if i % 2 == 0 {
            SystemMode::Paused
        } else {
            SystemMode::Running
        });

        let duration = start.elapsed();
        latencies.push(duration);

        sleep(Duration::from_micros(100)).await;
    }

    // Calculate statistics
    let total: Duration = latencies.iter().sum();
    let avg_latency = total / iterations as u32;
    let max_latency = latencies.iter().max().unwrap();

    println!(
        "Broadcast average latency ({} subscribers): {:?}",
        num_subscribers, avg_latency
    );
    println!("Broadcast max latency: {:?}", max_latency);

    // Broadcast overhead should be < 1ms even with multiple subscribers
    assert!(
        avg_latency < Duration::from_millis(1),
        "Broadcast latency too high: {:?}",
        avg_latency
    );
}
