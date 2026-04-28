use ghost_brain::{MarketSignals, MciConfig, MciEngine, MciInitialState};

#[test]
fn initial_state_force_override_sets_mci() {
    let mut config = MciConfig::default();
    config.initial_state = Some(MciInitialState {
        base_sentiment: 0.9,
        volatility_index: 0.3,
        force_override: true,
    });

    let engine = MciEngine::new(config);
    let mut signals = MarketSignals::mock();
    signals.flow.qass_alignment = -1.0;
    signals.flow.magnitude = 0.0;

    let result = engine.compute_mci(&signals);
    assert!((result.mci - 0.9).abs() < 0.01);
}

#[test]
fn initial_state_bias_without_override_prefills_memory() {
    let mut config = MciConfig::default();
    config.initial_state = Some(MciInitialState {
        base_sentiment: 0.8,
        volatility_index: 0.0,
        force_override: false,
    });

    let engine = MciEngine::new(config);
    let mut signals = MarketSignals::mock();
    signals.flow.qass_alignment = -1.0;
    signals.flow.magnitude = 0.0;

    let result = engine.compute_mci(&signals);
    assert!((result.mci - 0.8).abs() < 0.01);
}
