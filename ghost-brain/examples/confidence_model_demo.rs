//! Confidence Model Demonstration
//!
//! This example demonstrates how to use the Confidence Model to calculate
//! confidence scores for Oracle Brain decisions.

use ghost_brain::oracle::confidence_model::{ConfidenceInputs, ConfidenceModel, ConfidenceWeights};
use ghost_brain::signals::MarketSignals;

fn main() {
    println!("=== Confidence Model Demonstration ===\n");

    // Create a confidence model with default weights
    let model = ConfidenceModel::default();

    println!("Default Confidence Weights:");
    println!("  SOBP:        {:.1}", model.weights.sobp);
    println!("  MPCF:        {:.1}", model.weights.mpcf);
    println!("  IWIM:        {:.1}", model.weights.iwim);
    println!("  SSMI:        {:.1}", model.weights.ssmi);
    println!("  QASS:        {:.1}", model.weights.qass);
    println!("  QOFSV:       {:.1}", model.weights.qofsv);
    println!("  SCR:         {:.1}", model.weights.scr);
    println!("  FRB:         {:.1}", model.weights.frb);
    println!("  QMAN:        {:.1}", model.weights.qman);
    println!("  GeneMapper:  {:.1}", model.weights.gene_mapper);
    println!("  ChaosEngine: {:.1}", model.weights.chaos_engine);
    println!("  Total Weight: {:.1}\n", model.weights.total_weight());

    // Scenario 1: Perfect signals - high confidence
    println!("--- Scenario 1: Perfect Signals (High Confidence) ---");
    let perfect_inputs = ConfidenceInputs {
        sobp_drop: 0.0,
        sobp_current: 1.5,
        sobp_ma: 1.0,
        mpcf_entropy: 1.0,
        iwim_network_coherence: 1.0,
        iwim_bot_score: 0.0,
        ssmi_entropy: 1.0,
        qass_score: 100.0,
        qass_volatility: 0.0,
        qofsv_flow_magnitude: 1.0,
        qofsv_alignment_noise: 0.0,
        scr_score: 0.0,
        frb_flow_coherence: 1.0,
        frb_resonance_noise: 0.0,
        qman_deviation_risk: 0.0,
        gene_mapper_match_score: 0.0,
        chaos_loss_probability: 0.0,
    };

    let perfect_score = model.calculate_confidence(&perfect_inputs);
    println!("Overall Confidence: {:.3}", perfect_score.overall);
    println!("Module Contributions:");
    println!("  SOBP:        {:.3}", perfect_score.contributions.sobp);
    println!("  MPCF:        {:.3}", perfect_score.contributions.mpcf);
    println!("  IWIM:        {:.3}", perfect_score.contributions.iwim);
    println!("  SSMI:        {:.3}", perfect_score.contributions.ssmi);
    println!("  QASS:        {:.3}", perfect_score.contributions.qass);
    println!("  QOFSV:       {:.3}", perfect_score.contributions.qofsv);
    println!("  SCR:         {:.3}", perfect_score.contributions.scr);
    println!("  FRB:         {:.3}", perfect_score.contributions.frb);
    println!("  QMAN:        {:.3}", perfect_score.contributions.qman);
    println!(
        "  GeneMapper:  {:.3}",
        perfect_score.contributions.gene_mapper
    );
    println!(
        "  ChaosEngine: {:.3}",
        perfect_score.contributions.chaos_engine
    );
    println!("Metadata:");
    println!("  Valid Modules:  {}", perfect_score.metadata.valid_modules);
    println!(
        "  Data Quality:   {:.3}",
        perfect_score.metadata.data_quality
    );
    println!(
        "  Noise Level:    {:.3}\n",
        perfect_score.metadata.noise_level
    );

    // Scenario 2: Poor signals - low confidence
    println!("--- Scenario 2: Poor Signals (Low Confidence) ---");
    let poor_inputs = ConfidenceInputs {
        sobp_drop: 1.0,
        sobp_current: 0.5,
        sobp_ma: 2.0,
        mpcf_entropy: 0.1,
        iwim_network_coherence: 0.2,
        iwim_bot_score: 0.9,
        ssmi_entropy: 0.1,
        qass_score: 10.0,
        qass_volatility: 0.9,
        qofsv_flow_magnitude: 0.1,
        qofsv_alignment_noise: 0.9,
        scr_score: 0.9,
        frb_flow_coherence: 0.1,
        frb_resonance_noise: 0.9,
        qman_deviation_risk: 0.9,
        gene_mapper_match_score: 0.9,
        chaos_loss_probability: 0.9,
    };

    let poor_score = model.calculate_confidence(&poor_inputs);
    println!("Overall Confidence: {:.3}", poor_score.overall);
    println!("Module Contributions:");
    println!("  SOBP:        {:.3}", poor_score.contributions.sobp);
    println!("  MPCF:        {:.3}", poor_score.contributions.mpcf);
    println!("  IWIM:        {:.3}", poor_score.contributions.iwim);
    println!("  SSMI:        {:.3}", poor_score.contributions.ssmi);
    println!("  QASS:        {:.3}", poor_score.contributions.qass);
    println!("  QOFSV:       {:.3}", poor_score.contributions.qofsv);
    println!("  SCR:         {:.3}", poor_score.contributions.scr);
    println!("  FRB:         {:.3}", poor_score.contributions.frb);
    println!("  QMAN:        {:.3}", poor_score.contributions.qman);
    println!("  GeneMapper:  {:.3}", poor_score.contributions.gene_mapper);
    println!(
        "  ChaosEngine: {:.3}",
        poor_score.contributions.chaos_engine
    );
    println!("Metadata:");
    println!("  Valid Modules:  {}", poor_score.metadata.valid_modules);
    println!("  Data Quality:   {:.3}", poor_score.metadata.data_quality);
    println!("  Noise Level:    {:.3}\n", poor_score.metadata.noise_level);

    // Scenario 3: Mixed signals - medium confidence
    println!("--- Scenario 3: Mixed Signals (Medium Confidence) ---");
    let mixed_inputs = ConfidenceInputs {
        sobp_drop: 0.3,
        sobp_current: 1.2,
        sobp_ma: 1.0,
        mpcf_entropy: 0.6,
        iwim_network_coherence: 0.7,
        iwim_bot_score: 0.3,
        ssmi_entropy: 0.65,
        qass_score: 65.0,
        qass_volatility: 0.2,
        qofsv_flow_magnitude: 0.7,
        qofsv_alignment_noise: 0.25,
        scr_score: 0.3,
        frb_flow_coherence: 0.6,
        frb_resonance_noise: 0.3,
        qman_deviation_risk: 0.25,
        gene_mapper_match_score: 0.15,
        chaos_loss_probability: 0.2,
    };

    let mixed_score = model.calculate_confidence(&mixed_inputs);
    println!("Overall Confidence: {:.3}", mixed_score.overall);
    println!("Module Contributions:");
    println!("  SOBP:        {:.3}", mixed_score.contributions.sobp);
    println!("  MPCF:        {:.3}", mixed_score.contributions.mpcf);
    println!("  IWIM:        {:.3}", mixed_score.contributions.iwim);
    println!("  SSMI:        {:.3}", mixed_score.contributions.ssmi);
    println!("  QASS:        {:.3}", mixed_score.contributions.qass);
    println!("  QOFSV:       {:.3}", mixed_score.contributions.qofsv);
    println!("  SCR:         {:.3}", mixed_score.contributions.scr);
    println!("  FRB:         {:.3}", mixed_score.contributions.frb);
    println!("  QMAN:        {:.3}", mixed_score.contributions.qman);
    println!(
        "  GeneMapper:  {:.3}",
        mixed_score.contributions.gene_mapper
    );
    println!(
        "  ChaosEngine: {:.3}",
        mixed_score.contributions.chaos_engine
    );
    println!("Metadata:");
    println!("  Valid Modules:  {}", mixed_score.metadata.valid_modules);
    println!("  Data Quality:   {:.3}", mixed_score.metadata.data_quality);
    println!(
        "  Noise Level:    {:.3}\n",
        mixed_score.metadata.noise_level
    );

    // Scenario 4: Using custom weights
    println!("--- Scenario 4: Custom Weights (Emphasizing QASS and QMAN) ---");
    let custom_weights = ConfidenceWeights {
        qass: 25.0, // Increased from 15.0
        qman: 20.0, // Increased from 14.0
        ..Default::default()
    };
    let custom_model = ConfidenceModel::with_weights(custom_weights);

    let custom_score = custom_model.calculate_confidence(&mixed_inputs);
    println!(
        "Overall Confidence (custom weights): {:.3}",
        custom_score.overall
    );
    println!(
        "Compare to default weights:          {:.3}",
        mixed_score.overall
    );
    println!(
        "Difference:                          {:.3}\n",
        custom_score.overall - mixed_score.overall
    );

    // Scenario 5: Integration with MarketSignals
    println!("--- Scenario 5: Integration with MarketSignals ---");
    let signals = MarketSignals::mock();

    let inputs_from_signals = ConfidenceModel::build_inputs_from_signals(
        &signals, 75.0, // qass_score
        0.15, // qass_volatility
        0.2,  // scr_score
        0.05, // gene_mapper_score
        0.12, // chaos_loss_prob
    );

    let signal_score = model.calculate_confidence(&inputs_from_signals);
    println!(
        "Overall Confidence from MarketSignals: {:.3}",
        signal_score.overall
    );
    println!("Module Contributions:");
    println!("  SOBP:        {:.3}", signal_score.contributions.sobp);
    println!("  MPCF:        {:.3}", signal_score.contributions.mpcf);
    println!("  IWIM:        {:.3}", signal_score.contributions.iwim);
    println!("  SSMI:        {:.3}", signal_score.contributions.ssmi);
    println!("  QASS:        {:.3}", signal_score.contributions.qass);
    println!("  QOFSV:       {:.3}", signal_score.contributions.qofsv);
    println!("  SCR:         {:.3}", signal_score.contributions.scr);
    println!("  FRB:         {:.3}", signal_score.contributions.frb);
    println!("  QMAN:        {:.3}", signal_score.contributions.qman);
    println!(
        "  GeneMapper:  {:.3}",
        signal_score.contributions.gene_mapper
    );
    println!(
        "  ChaosEngine: {:.3}\n",
        signal_score.contributions.chaos_engine
    );

    // Decision guidance based on confidence
    println!("--- Decision Guidance Based on Confidence ---");
    for (scenario, score) in &[
        ("Perfect", perfect_score.overall),
        ("Poor", poor_score.overall),
        ("Mixed", mixed_score.overall),
        ("From Signals", signal_score.overall),
    ] {
        let guidance = if *score > 0.8 {
            "HIGH CONFIDENCE - Full conviction, normal position sizing"
        } else if *score > 0.5 {
            "MEDIUM CONFIDENCE - Moderate conviction, reduced position"
        } else {
            "LOW CONFIDENCE - Low conviction, skip or minimal position"
        };
        println!("  {:<15} ({:.3}): {}", scenario, score, guidance);
    }

    println!("\n=== Demonstration Complete ===");
}
