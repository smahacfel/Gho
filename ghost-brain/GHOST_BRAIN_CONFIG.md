# Ghost Brain Configuration System

## Overview

The Ghost Brain unified configuration system provides a centralized way to configure and tune all components of the Ghost Brain trading system. Instead of having parameters scattered across multiple files with hardcoded constants, all tunable parameters are now organized in a single configuration structure.

## Architecture

```
ghost-brain/
├── src/
│   └── config/
│       ├── mod.rs                      # Module exports
│       ├── ghost_brain_config.rs       # Unified configuration (NEW)
│       ├── qedd_config.rs              # QEDD-specific config
│       ├── mci_config.rs               # MCI-specific config
│       └── e2e_config.rs               # E2E pipeline config
├── ghost_brain_config.example.json     # Example JSON config
└── ghost_brain_config.example.toml     # Example TOML config
```

## Components Covered

The unified `GhostBrainConfig` structure covers all major Ghost Brain components:

### 1. **MPCF** (Micro-Payload Cognitive Fingerprint)
Actor-behavioral byte fingerprinting for ultra-fast classification of transaction sources.

**Key Parameters:**
- `bot_entropy_threshold`: Entropy threshold for bot detection (default: 3.5)
- `human_entropy_threshold`: Entropy threshold for human detection (default: 5.5)
- `bot_iss_variance_threshold`: Instruction spacing variance for bots (default: 0.15)
- `min_payload_size`: Minimum bytes for reliable analysis (default: 32)
- `max_payload_size`: Maximum bytes to analyze (default: 4096)

### 2. **SSMI** (Sub-Slot Microentropy Index)
Transaction timing jitter analysis for source classification.

**Key Parameters:**
- `bot_scr_threshold`: SCR probability threshold for bots (default: 0.7)
- `bot_ar_threshold`: AR correlation threshold for bots (default: 0.8)
- `bot_entropy_threshold`: Entropy threshold for bots (default: 1.5)
- `human_entropy_threshold`: Entropy threshold for humans (default: 3.0)
- `viral_min_tx_count`: Minimum transactions for viral detection (default: 6)
- `score_weight_entropy`: Weight for entropy in scoring (default: 0.35)
- `score_weight_scr`: Weight for SCR in scoring (default: 0.40)
- `score_weight_ar`: Weight for AR in scoring (default: 0.25)

### 3. **IWIM** (Initial Wallet Intent Mapping)
Dev-wallet behavioral analysis for detecting creator intentions.

**Key Parameters:**
- `iapp_rug_threshold`: Token accounts created threshold for rug flag (default: 2)
- `min_iapp_rug_score`: Minimum rug score when threshold met (default: 0.95)
- `at_window_ms`: Authority change window in milliseconds (default: 1500)
- `quiet_window_ms`: Pre-mint quietness window (default: 5000)
- `confidence_threshold`: Threshold for reliable classification (default: 0.6)

### 4. **QASS** (Quantum Amplitude Superposition Scoring)
Quantum-inspired signal aggregation engine.

**Key Parameters:**
- `collapse_threshold`: Wave function collapse threshold (default: 0.5)
- `score_threshold_viral`: Score threshold for viral classification (default: 0.85)
- `score_threshold_moderate`: Score threshold for moderate bullish (default: 0.70)
- `score_threshold_neutral`: Score threshold for neutral (default: 0.50)
- `score_threshold_suspicious`: Score threshold for suspicious (default: 0.30)

### 5. **SOBP** (Slot-Over-Slot Buying Pressure)
Buying pressure analysis for pump onset detection.

**Key Parameters:**
- `human_weight_multiplier`: Weight for human actors (default: 2.0)
- `sniper_weight_multiplier`: Weight for sniper bots (default: 0.5)
- `hyper_pump_threshold`: Threshold for hyper-aggressive buy influx (default: 3.0)
- `growth_threshold`: Threshold for stable organic growth (default: 1.5)
- `stagnation_threshold`: Threshold for stagnation (default: 0.8)
- `implosion_threshold`: Threshold for demand implosion (default: 0.4)
- `slot_capacity`: Slot history buffer capacity (default: 64)

### 6. **QOFSV** (Quantum Order-Flow State Vector)
Quantum state vector mapping from market signals.

**Key Parameters:**
- `state_vector_dim`: Number of features in state vector (default: 6)
- `epsilon`: Numerical stability epsilon (default: 1e-6)
- `target_construction_time_us`: Performance target for construction (default: 200μs)

### 7. **FRB** (Frequency Resonance Bands)
Frequency-based signal analysis.

**Key Parameters:**
- `min_amplitude_threshold`: Minimum amplitude for signal detection (default: 0.001)
- `enable_filtering`: Enable advanced frequency filtering (default: true)

### 8. **Resonance**
Bot detection via coefficient of variation analysis.

**Key Parameters:**
- `bot_threshold_cv`: CV threshold for bot detection (default: 0.3)
- `human_threshold_cv`: CV threshold for human detection (default: 0.8)

## Usage

### Loading Configuration

#### From Default Values
```rust
use ghost_brain::config::GhostBrainConfig;

let config = GhostBrainConfig::default();
```

#### From JSON File
```rust
use ghost_brain::config::GhostBrainConfig;

let config = GhostBrainConfig::from_json_file("ghost_brain_config.json")?;
```

#### From TOML File
```rust
use ghost_brain::config::GhostBrainConfig;

let config = GhostBrainConfig::from_toml_file("ghost_brain_config.toml")?;
```

### Customizing Configuration

```rust
use ghost_brain::config::GhostBrainConfig;

let mut config = GhostBrainConfig::default();

// Adjust MPCF parameters for stricter bot detection
config.mpcf.bot_entropy_threshold = 4.0;
config.mpcf.bot_iss_variance_threshold = 0.12;

// Adjust SOBP thresholds for more aggressive pump detection
config.sobp.hyper_pump_threshold = 2.5;
config.sobp.growth_threshold = 1.3;

// Adjust IWIM for stricter rug detection
config.iwim.iapp_rug_threshold = 1;
config.iwim.min_iapp_rug_score = 0.98;

// Validate configuration
config.validate()?;
```

### Saving Configuration

```rust
use ghost_brain::config::GhostBrainConfig;

let config = GhostBrainConfig::default();

// Save to JSON
config.to_json_file("my_config.json")?;

// Save to TOML
config.to_toml_file("my_config.toml")?;
```

## Configuration Files

### JSON Format
See `ghost_brain_config.example.json` for a complete example in JSON format.

```json
{
  "version": 1,
  "mpcf": {
    "bot_entropy_threshold": 3.5,
    "human_entropy_threshold": 5.5,
    ...
  },
  "ssmi": {
    "bot_scr_threshold": 0.7,
    ...
  }
}
```

### TOML Format
See `ghost_brain_config.example.toml` for a complete example in TOML format with detailed comments.

```toml
version = 1

[mpcf]
bot_entropy_threshold = 3.5
human_entropy_threshold = 5.5
...

[ssmi]
bot_scr_threshold = 0.7
...
```

## Validation

The configuration system includes comprehensive validation to ensure all parameters are within valid ranges:

```rust
let config = GhostBrainConfig::from_json_file("config.json")?;
config.validate()?; // Automatically called during file loading
```

Validation checks include:
- All thresholds are within specified ranges
- Weights sum to 1.0 where required (e.g., SSMI score weights)
- Slot capacities are sufficient for minimum history requirements
- All required parameters are present

## Tuning Guidelines

### For Conservative Trading
- Increase `mpcf.bot_entropy_threshold` → Stricter bot detection
- Increase `sobp.hyper_pump_threshold` → Require stronger buying pressure
- Increase `qass.score_threshold_moderate` → Higher bar for entry signals
- Decrease `iwim.iapp_rug_threshold` → More sensitive rug detection

### For Aggressive Trading
- Decrease `mpcf.bot_entropy_threshold` → More lenient bot classification
- Decrease `sobp.hyper_pump_threshold` → Enter on weaker signals
- Decrease `qass.score_threshold_moderate` → Lower bar for entry
- Increase `iwim.iapp_rug_threshold` → Less sensitive rug detection

### For High-Volume Markets
- Increase `sobp.slot_capacity` → Track longer history
- Increase `ssmi.histogram_bins` → Finer timing granularity
- Decrease `sobp.human_weight_multiplier` → Reduce organic bias
- Increase `viral_min_tx_count` → Require more transactions for viral detection

### For Low-Volume Markets
- Decrease `sobp.min_slot_history` → Work with less data
- Increase `sobp.human_weight_multiplier` → Amplify organic signals
- Decrease `viral_min_tx_count` → Detect viral launches earlier

## Migration from Hardcoded Constants

The unified configuration system replaces hardcoded constants in the following files:

- `src/oracle/ultrafast/mpcf.rs` → `MpcfConfig`
- `src/oracle/ultrafast/ssmi.rs` → `SsmiConfig`
- `src/oracle/ultrafast/iwim.rs` → `IwimConfig`
- `src/oracle/ultrafast/qass.rs` → `QassConfig`
- `src/oracle/ultrafast/sobp.rs` → `SobpConfig`
- `src/oracle/ultrafast/qofsv.rs` → `QofsvConfig`
- `src/signals/frb.rs` → `FrbConfig`
- `src/signals/resonance.rs` → `ResonanceConfig`

To integrate the new configuration system into components:

1. Import the configuration: `use crate::config::GhostBrainConfig;`
2. Accept config as parameter: `fn analyze(data: &[u8], config: &MpcfConfig) -> Result<...>`
3. Use config values instead of constants: `config.bot_entropy_threshold` instead of `BOT_ENTROPY_THRESHOLD`

## Testing

The configuration module includes comprehensive unit tests:

```bash
cd ghost-brain
cargo test -p ghost-brain config::ghost_brain_config::tests
```

Tests cover:
- Default value initialization
- Serialization/deserialization (JSON and TOML)
- Validation logic for all parameters
- Weight sum validation (SSMI, SOBP)
- Range validation for thresholds

## Best Practices

1. **Always validate** configuration after loading from files
2. **Use version field** to handle API compatibility across config versions
3. **Document custom values** when deviating from defaults
4. **Test configuration changes** in simulation before production
5. **Keep backups** of working configurations
6. **Monitor performance** when adjusting thresholds
7. **Use TOML for human editing** (comments, readability)
8. **Use JSON for programmatic** generation/manipulation

## Future Enhancements

Potential future additions to the configuration system:

- [ ] Environment variable overrides for specific parameters
- [ ] Hot-reload capability for runtime configuration updates
- [ ] Configuration profiles (conservative, balanced, aggressive)
- [ ] A/B testing framework for parameter optimization
- [ ] Configuration history tracking and rollback
- [ ] Web UI for visual parameter tuning
- [ ] Machine learning-based parameter optimization

## Support

For questions or issues with the configuration system:
1. Review the example configuration files
2. Check validation error messages for specific issues
3. Consult component documentation for parameter meanings
4. Review test cases for usage examples
