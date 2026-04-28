//! QEDD and MCI Integration Example
//!
//! Demonstrates the integration of QEDD (Quantum Entropy-Driven Decay) and
//! MCI (Market Coherence Index) with the HyperPrediction Oracle.
//!
//! ## What this demonstrates:
//! - Creating a HyperPredictionOracle with QEDD and MCI engines
//! - Scoring a token candidate
//! - Inspecting QEDD and MCI results
//! - Writing results to JSONL dataset files
//! - Understanding veto conditions

use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_brain::oracle::HyperPredictionOracle;
use ghost_brain::pumpfun::PumpCurveStateCache;
use ghost_brain::telemetry::DatasetWriter;
use solana_sdk::pubkey::Pubkey;

#[tokio::main]
async fn main() {
    println!("=== QEDD and MCI Integration Demo ===\n");

    // Create HyperPrediction Oracle with QEDD and MCI engines
    let oracle = HyperPredictionOracle::new(70); // 70 = threshold score
    println!("✓ Created HyperPrediction Oracle with QEDD and MCI engines");

    // Pump.fun snapshot cache (required by score_candidate after PRAECOG integration)
    let pumpfun_cache = PumpCurveStateCache::new();

    // Create a mock candidate for demonstration
    let candidate = create_mock_candidate();
    println!("✓ Created mock token candidate");
    println!("  - Pool: {}", candidate.pool_amm_id);
    println!("  - Liquidity: {} SOL", candidate.initial_liquidity_sol);
    println!(
        "  - Bonding Curve Progress: {:.2}%\n",
        candidate.bonding_curve_progress.unwrap_or(0.0) * 100.0
    );

    // Score the candidate
    println!("Scoring candidate...");
    let result = oracle
        .score_candidate(
            &candidate,
            &pumpfun_cache, // NEW: required parameter
            None,           // explicit_pool_state
            None,           // tx_timestamps
            None,           // tx_data
            None,           // iwim_result
            None,           // chaos_result
            None,           // resonance_result
            None,           // gene_safety_result
            None,           // hunter_score
            None,           // tx_metrics
            None,           // cluster_result
            None,           // paradox_state
            None,           // tuned_weights
            None,           // ligma_result
        )
        .expect("Failed to score candidate");

    // Display results
    println!("\n=== Scoring Results ===");
    println!("Final Score: {} / 100", result.score);
    println!("Passed Threshold: {}", result.passed);
    println!("Risk Level: {:?}", result.risk_level);
    println!("Processing Time: {}μs\n", result.processing_time_us);

    // Display QEDD results
    if let Some(ref qedd) = result.qedd_result {
        println!("=== QEDD (Quantum Entropy-Driven Decay) ===");
        println!("Lambda (decay rate): {:.3}", qedd.lambda_now);
        println!("Survival Probabilities:");
        println!(
            "  - 1 second:  {:.4} ({:.1}%)",
            qedd.survival_1s,
            qedd.survival_1s * 100.0
        );
        println!(
            "  - 5 seconds: {:.4} ({:.1}%)",
            qedd.survival_5s,
            qedd.survival_5s * 100.0
        );
        println!("  - 30 seconds: {:.6}", qedd.survival_30s);
        println!("  - 60 seconds: {:.6}", qedd.survival_60s);
        println!("Computation Time: {}ms", qedd.computation_ms);

        // Check veto condition
        let lambda_threshold = 0.95; // Default threshold
        if qedd.lambda_now > lambda_threshold {
            println!(
                "⚠️  VETO: Lambda exceeds threshold ({:.3} > {:.3})",
                qedd.lambda_now, lambda_threshold
            );
        } else {
            println!("✓ Lambda within acceptable range");
        }
        println!();
    }

    // Display MCI results
    if let Some(ref mci) = result.mci_result {
        println!("=== MCI (Market Coherence Index) ===");
        println!("MCI Value: {:.3}", mci.mci);
        println!("  - Directional Coherence (DC): {:.3}", mci.dc);
        println!("  - Structural Coherence (SC): {:.3}", mci.sc);
        println!("Computation Time: {}ms", mci.computation_ms);

        // Check veto condition
        let coherence_threshold = 0.3; // Default threshold
        if mci.mci < coherence_threshold {
            println!(
                "⚠️  VETO: MCI below threshold ({:.3} < {:.3})",
                mci.mci, coherence_threshold
            );
        } else {
            println!("✓ MCI above acceptable threshold");
        }
        println!();
    }

    // Display interpretation
    println!("=== Interpretation ===");
    println!("{}\n", result.interpretation);

    // Write results to dataset files
    println!("=== Writing to Dataset ===");
    let writer = DatasetWriter::new();
    let session_id = match candidate.slot {
        Some(slot) => format!("slot_{}_demo", slot),
        None => "slot_unknown_demo".to_string(),
    };

    if let (Some(qedd), Some(mci)) = (&result.qedd_result, &result.mci_result) {
        match writer.write_both(&session_id, qedd, mci).await {
            Ok((qedd_path, mci_path)) => {
                println!("✓ Wrote QEDD results to: {:?}", qedd_path);
                println!("✓ Wrote MCI results to: {:?}", mci_path);
            }
            Err(e) => {
                println!("⚠️  Failed to write dataset: {}", e);
            }
        }
    }

    println!("\n=== Demo Complete ===");
    println!("\nKey Insights:");
    println!("- QEDD measures token survival probability over time");
    println!("- High lambda (decay rate) indicates high risk");
    println!("- MCI measures market coherence (directional + structural)");
    println!("- Low MCI indicates incoherent/suspicious market behavior");
    println!("- Both QEDD and MCI contribute 15% each to final score (base score = 70%)");
    println!("- Veto conditions can reject tokens regardless of base score");
}

/// Create a mock enhanced candidate for demonstration
fn create_mock_candidate() -> EnhancedCandidate {
    EnhancedCandidate::new_with_fields(
        // Hot fields
        Some(123456789), // slot
        1234567890000,   // timestamp
        12.5,            // initial_liquidity_sol
        2.5,             // dev_buy_sol
        Some(0.15),      // bonding_curve_progress
        65,              // vanity_score
        75,              // metadata_len_score
        true,            // has_dev_buy
        true,            // mint_auth_disabled
        // Shadow fields
        None,                 // expected_price
        Some(15),             // shadow_bonding_progress
        Some(42_500_000_000), // virtual_sol_reserves
        None,                 // shadow_market_cap
        // Cold fields
        Pubkey::new_unique(), // pool_amm_id
        "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
            .parse()
            .unwrap(), // amm_program_id
        Pubkey::new_unique(), // base_mint
        Pubkey::new_unique(), // quote_mint
        Pubkey::new_unique(), // bonding_curve
        "demo_signature_123456".to_string(), // signature
        Some(1_000_000_000),  // token_total_supply
    )
}
