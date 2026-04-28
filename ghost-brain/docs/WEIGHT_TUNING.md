# Weight Tuning Mechanisms

## Overview

Ghost Brain implements two complementary weight tuning mechanisms for optimizing signal weights
that control trading decisions. These mechanisms adapt the system to changing market conditions
and improve trading performance over time.

### Tunable Weights

The following weights are subject to automatic tuning:

| Weight | Description | Default | Range |
|--------|-------------|---------|-------|
| `wQASS` | Quantum Amplitude Scoring System | 15.0 | [1.0, 30.0] |
| `wMPCF` | Micro-Payload Cognitive Fingerprint | 10.0 | [1.0, 30.0] |
| `wSOBP` | Slot-Over-Slot Buying Pressure | 12.0 | [1.0, 30.0] |
| `wIWIM` | Inter-Wallet Interaction Matrix | 8.0 | [1.0, 30.0] |

---

## 1. Online Bandits (Real-time Tuning)

### Purpose

Online bandit algorithms provide continuous, real-time weight adjustment based on immediate
trading outcomes. Updates occur every 1-5 minutes to quickly adapt to market changes.

### Algorithms

#### LinUCB (Linear Upper Confidence Bound)

```
UCB(a) = θᵀx + α√(xᵀA⁻¹x)
```

Where:
- `θ` = learned weight vector
- `x` = context features
- `α` = exploration parameter
- `A` = design matrix (accumulated context outer products)

**Pros:**
- Theoretically bounded regret
- Handles context features well
- Deterministic given the same context

**Cons:**
- Requires careful exploration parameter tuning
- Can be slow to explore in high dimensions

#### Thompson Sampling

Samples weights from posterior distributions:

```
For each weight w_i:
    sample ~ Normal(μᵢ, σᵢ²)
    where μᵢ, σᵢ² are posterior parameters
```

**Pros:**
- Natural exploration through sampling
- Often performs better empirically
- Handles non-stationary environments

**Cons:**
- Stochastic (can be inconsistent)
- Requires more computation

### Context Features

The bandit algorithms use the following context features for decision-making:

| Feature | Description | Range |
|---------|-------------|-------|
| `volatility` | Market volatility indicator | [0.0, 1.0] |
| `volume` | Normalized trading volume | [0.0, 1.0] |
| `bot_activity` | Bot activity level | [0.0, 1.0] |
| `mci` | Market Coherence Index | [0.0, 1.0] |
| `time_factor` | Time-of-day pattern | [0.0, 1.0] |
| `pool_age` | Normalized pool age | [0.0, 1.0] |

### Reward Signal

The reward signal is computed from trade outcomes:

```
reward = profit_component + penalty_component
```

**Profit Component:**
- Success: `success_reward + profit_scale × profit%`
- Loss: `-loss_scale × loss%`

**Penalty Component:**
- False positive (bad BUY): `false_positive_penalty` (-0.5 default)
- False negative (missed opportunity): `false_negative_penalty` (-0.2 default)

### Configuration

```toml
[tuning.bandit]
update_interval_seconds = 180  # 3 minutes
learning_rate = 0.1
exploration_alpha = 1.0
min_weight = 1.0
max_weight = 30.0
target_weight_sum = 45.0
discount_factor = 0.95
```

---

## 2. Offline Bayesian Optimization (12h Cycle)

### Purpose

Bayesian optimization performs comprehensive hyperparameter optimization using historical data.
Runs on a 12-hour cycle for thorough exploration of the parameter space without impacting
real-time performance.

### Target Parameters

Beyond the core weights, Bayesian optimization can tune:
- QASS amplitude parameters
- Confidence thresholds
- QEDD decay rates
- MCI weight distributions

### Algorithm

Uses Gaussian Process (GP) surrogate model with acquisition function optimization:

#### Gaussian Process

```
f(x) ~ GP(μ(x), k(x, x'))
```

With RBF (Radial Basis Function) kernel:

```
k(x, x') = σ² exp(-||x - x'||² / 2l²)
```

Where:
- `σ²` = signal variance
- `l` = length scale

#### Acquisition Functions

**Expected Improvement (default):**

```
EI(x) = (μ(x) - f_best - ξ)Φ(z) + σ(x)φ(z)
```

**Upper Confidence Bound:**

```
UCB(x) = μ(x) + β^(1/2) × σ(x)
```

**Probability of Improvement:**

```
PI(x) = Φ((μ(x) - f_best - ξ) / σ(x))
```

### Configuration

```toml
[tuning.bayesian]
enabled = true
cycle_duration = 43200  # 12 hours in seconds
n_initial_points = 10
n_iterations = 50
n_random_samples = 100
acquisition_function = "ExpectedImprovement"
beta = 2.0
xi = 0.01
gp_length_scale = 1.0
gp_signal_variance = 1.0
gp_noise_variance = 0.01
blend_factor = 0.3
```

### Optimization Cycle

1. **Data Collection**: Gather 12 hours of trading outcomes
2. **GP Fitting**: Build surrogate model from historical data
3. **Acquisition Optimization**: Find optimal next evaluation point
4. **Evaluation**: Test weight configuration on historical data
5. **Repeat**: Continue until budget exhausted
6. **Apply**: Blend optimized weights with conservative factor

---

## 3. Fallback: Manual Parameter Freezing

### Purpose

Emergency mechanism to lock parameters when automatic tuning produces undesirable results
or when manual intervention is required.

### Triggers

Parameters are frozen when:
1. **Drift Detection**: Cumulative reward drops below threshold
2. **Manual Override**: Operator intervenes via TOML file
3. **Sudden Performance Drop**: Recent performance significantly worse than earlier

### Configuration

```toml
[tuning.frozen]
file_path = "frozen_params.toml"
auto_freeze_on_drift = true
drift_threshold = -2.0
drift_window = 100
```

### Manual Freeze Format

Create a `frozen_params.toml` file:

```toml
enabled = true
reason = "Manual intervention due to market volatility"

[weights]
w_qass = 15.0
w_mpcf = 10.0
w_sobp = 12.0
w_iwim = 8.0
```

### Drift Detection Algorithm

```
if avg_reward(last_N) < drift_threshold:
    freeze(current_weights)
    
if recent_avg < earlier_avg × 0.5:
    freeze(current_weights)  # Sudden drop
```

---

## Integration

### Telemetry

All weight updates are logged to telemetry for analysis:

```json
{
  "event_type": "WeightUpdate",
  "timestamp_ms": 1700000000000,
  "weights": {
    "w_qass": 15.2,
    "w_mpcf": 10.1,
    "w_sobp": 11.8,
    "w_iwim": 7.9
  },
  "reward": 0.85,
  "algorithm": "LinUCB"
}
```

### Dry Run Mode

Tuning works in simulation mode:
- Updates are calculated but not applied to live trading
- Allows validation of tuning strategy before deployment
- Historical replay supported for backtesting

### Historical Replay

Replay historical data for:
- Backtesting different tuning configurations
- Comparing LinUCB vs Thompson Sampling performance
- Validating Bayesian optimization improvements

---

## Usage Examples

### Basic Usage

```rust
use ghost_brain::tuning::{WeightTuner, TuningConfig, BanditAlgorithm};

// Create tuner with LinUCB
let config = TuningConfig::default();
let mut tuner = WeightTuner::new(config, BanditAlgorithm::LinUCB);

// Update based on trade outcome
let context = TuningContext {
    volatility: 0.5,
    volume: 0.7,
    bot_activity: 0.2,
    mci: 0.8,
    ..Default::default()
};

let outcome = TradeOutcome::success(1.1, 0.85);  // 10% profit
tuner.update(&context, &outcome);

// Get current optimized weights
let weights = tuner.current_weights();
println!("QASS: {}, MPCF: {}", weights.w_qass, weights.w_mpcf);
```

### Running Bayesian Optimization

```rust
// Collect historical data
let historical_data: Vec<(TuningContext, TradeOutcome)> = load_historical_data();

// Run optimization
if tuner.should_run_bayesian() {
    let result = tuner.run_bayesian_optimization(&historical_data);
    if let Some(opt_result) = result {
        println!("Best weights: {:?}", opt_result.best_weights);
        println!("Expected improvement: {}", opt_result.expected_improvement);
    }
}
```

### Manual Freeze

```rust
use ghost_brain::tuning::{FrozenParameters, TunableWeights};

let frozen = FrozenParameters::new(
    TunableWeights {
        w_qass: 15.0,
        w_mpcf: 10.0,
        w_sobp: 12.0,
        w_iwim: 8.0,
    },
    "Emergency freeze due to market volatility"
);

tuner.freeze_parameters(frozen);

// Later, unfreeze
tuner.unfreeze_parameters();
```

---

## Validation

### Comparing Approaches

Run validation to compare effectiveness:

```rust
use ghost_brain::tuning::{WeightTuner, TuningConfig, BanditAlgorithm};

let data = load_test_data();

// Test LinUCB
let mut linucb_tuner = WeightTuner::new(TuningConfig::default(), BanditAlgorithm::LinUCB);
let linucb_reward = run_backtest(&mut linucb_tuner, &data);

// Test Thompson Sampling
let mut ts_tuner = WeightTuner::new(TuningConfig::default(), BanditAlgorithm::ThompsonSampling);
let ts_reward = run_backtest(&mut ts_tuner, &data);

println!("LinUCB cumulative reward: {}", linucb_reward);
println!("Thompson Sampling cumulative reward: {}", ts_reward);
```

### Stability Metrics

Monitor tuning stability:
- Weight variance over time
- Update frequency
- Drift detection rate
- Freeze frequency

---

## Performance Considerations

- **Memory**: O(n²) for LinUCB design matrix, O(n) for Thompson Sampling
- **Latency**: < 1ms per online update, minutes for Bayesian optimization
- **CPU**: Minimal for bandits, moderate for GP fitting

---

## Troubleshooting

### Weights Not Converging

1. Increase `exploration_alpha` for more exploration
2. Check reward signal is informative
3. Verify context features are meaningful

### Frequent Freezing

1. Increase `drift_threshold` (less sensitive)
2. Increase `drift_window` (longer averaging)
3. Disable `auto_freeze_on_drift` temporarily

### Bayesian Optimization Poor Results

1. Increase `n_initial_points` for better GP fit
2. Increase `n_iterations` for more thorough search
3. Adjust GP hyperparameters (`gp_length_scale`, `gp_signal_variance`)
