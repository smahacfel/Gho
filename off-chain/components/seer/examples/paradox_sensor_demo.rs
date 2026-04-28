//! Paradox Sensor (EchoScanner) Usage Example
//!
//! This example demonstrates how to:
//! 1. Initialize and run the Paradox Sensor
//! 2. Monitor network telemetry in real-time
//! 3. Detect HFT anomalies and adjust trading strategy
//!
//! Run with: `cargo run --example paradox_sensor_demo`

use seer::paradox_sensor::{ParadoxSensor, ParadoxState};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🔮 Paradox Sensor (EchoScanner) Demo");
    println!("=====================================\n");

    // 1. Create Paradox Sensor
    let (sensor, mut state_rx) = ParadoxSensor::new();
    let sensor = Arc::new(sensor);

    println!("✅ Paradox Sensor initialized");

    // 2. Start background analysis loop
    let sensor_for_loop = Arc::clone(&sensor);
    tokio::spawn(async move {
        ParadoxSensor::run_analysis_loop(sensor_for_loop).await;
    });

    println!("✅ Analysis loop started (refreshing every 50ms)\n");

    // 3. Simulate network traffic (in a real scenario, this would be WebSocket messages)
    let sensor_for_traffic = Arc::clone(&sensor);
    tokio::spawn(async move {
        simulate_network_traffic(sensor_for_traffic).await;
    });

    // 4. Monitor state changes and react to anomalies
    println!("📊 Monitoring network telemetry...\n");

    let mut last_anomaly_detected = false;
    let mut sample_count = 0;

    loop {
        // Wait for state changes
        if state_rx.changed().await.is_ok() {
            let state = *state_rx.borrow();
            sample_count += 1;

            // Print state every 20 samples (about once per second at 50ms refresh)
            if sample_count % 20 == 0 {
                print_state(&state, sample_count);
            }

            // Detect anomaly transitions
            if state.anomaly_detected && !last_anomaly_detected {
                println!("\n⚠️  ANOMALY DETECTED!");
                println!("    Market tension: {:.2}%", state.tension);
                println!("    Jitter: {:.2}ms", state.jitter_ms);
                println!("    Density: {:.0} packets/sec", state.density_bps);
                println!("    → Recommended action: Increase Jito tip or pause trading\n");
            } else if !state.anomaly_detected && last_anomaly_detected {
                println!("\n✅ Market normalized (tension below threshold)\n");
            }

            last_anomaly_detected = state.anomaly_detected;
        }

        // Run for 30 seconds then exit
        if sample_count > 600 {
            break;
        }
    }

    println!("\n🎯 Demo completed. Paradox Sensor successfully detected network patterns.");

    Ok(())
}

/// Simulate realistic network traffic patterns
async fn simulate_network_traffic(sensor: Arc<ParadoxSensor>) {
    let mut phase = 0;

    loop {
        // Simulate different traffic patterns every 10 seconds
        let phase_duration = Duration::from_secs(10);
        let start = tokio::time::Instant::now();

        match phase {
            0 => {
                // Normal traffic: ~50-100 packets/sec with natural jitter
                println!("📡 Phase 1: Normal market activity (low density, natural jitter)");
                while start.elapsed() < phase_duration {
                    sensor.record_pulse(rand::random::<usize>() % 500 + 200);
                    sleep(Duration::from_millis(10 + rand::random::<u64>() % 20)).await;
                }
            }
            1 => {
                // Increased activity: ~200 packets/sec
                println!("📡 Phase 2: Increased activity (medium density)");
                while start.elapsed() < phase_duration {
                    sensor.record_pulse(rand::random::<usize>() % 800 + 300);
                    sleep(Duration::from_millis(5 + rand::random::<u64>() % 10)).await;
                }
            }
            2 => {
                // HFT Bot attack: Very high density (~500+ packets/sec) with low jitter
                println!("📡 Phase 3: SIMULATING HFT BOT ATTACK (high density, low jitter)");
                while start.elapsed() < phase_duration {
                    sensor.record_pulse(rand::random::<usize>() % 1000 + 500);
                    // Synchronized timing - simulates bot behavior
                    sleep(Duration::from_millis(2)).await;
                }
            }
            _ => {
                // Return to normal
                phase = -1; // Will wrap to 0 after increment
            }
        }

        phase += 1;
    }
}

/// Print current state in a formatted way
fn print_state(state: &ParadoxState, sample: usize) {
    let status = if state.anomaly_detected {
        "🔴 ALERT"
    } else {
        "🟢 NORMAL"
    };

    println!(
        "[Sample {:04}] {} | Tension: {:5.2}% | Jitter: {:6.2}ms | Density: {:6.0} pps",
        sample, status, state.tension, state.jitter_ms, state.density_bps
    );
}

/// Usage in Trigger decision logic
#[allow(dead_code)]
fn example_trigger_integration(paradox_state: ParadoxState, base_tip: f64) -> f64 {
    // Adjust Jito tip based on market tension
    if paradox_state.anomaly_detected {
        // High tension = likely HFT activity = need higher tip to compete
        let tension_multiplier = 1.0 + (paradox_state.tension / 100.0);
        let adjusted_tip = base_tip * tension_multiplier;

        println!(
            "💰 Adjusting tip: {:.4} SOL → {:.4} SOL (tension: {:.2}%)",
            base_tip, adjusted_tip, paradox_state.tension
        );

        adjusted_tip
    } else {
        base_tip
    }
}
