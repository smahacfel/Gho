//! Feature Catalog - Complete registry of all 200+ features
//!
//! This module defines all features available in the ML Feature Engineering Framework.
//! Features are organized into categories for better management and documentation.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Feature category for organization and selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FeatureCategory {
    /// Price-based features (current, historical, momentum)
    Price,
    /// Volume-based features (trading volume, velocity)
    Volume,
    /// Liquidity features (pool depth, slippage)
    Liquidity,
    /// Holder and distribution features
    Holders,
    /// Technical indicators (RSI, MACD, etc.)
    Technical,
    /// Social and sentiment features
    Social,
    /// On-chain metrics (transactions, wallet activity)
    OnChain,
    /// Time-series derived features
    TimeSeries,
    /// Cross-feature interactions
    Interaction,
    /// Market context features
    Market,
}

/// Complete feature identifier with metadata
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeatureId {
    /// Unique feature name
    pub name: String,
    /// Feature category
    pub category: FeatureCategory,
    /// Feature version (for tracking changes)
    pub version: u32,
}

impl FeatureId {
    pub fn new(name: &str, category: FeatureCategory) -> Self {
        Self {
            name: name.to_string(),
            category,
            version: 1,
        }
    }
}

impl fmt::Display for FeatureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}::{}_v{}", self.category, self.name, self.version)
    }
}

/// Feature metadata for documentation and management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureMetadata {
    /// Feature identifier
    pub id: FeatureId,
    /// Human-readable description
    pub description: String,
    /// Expected value range (min, max)
    pub value_range: (f64, f64),
    /// Whether feature requires normalization
    pub requires_normalization: bool,
    /// Importance rank (higher = more important)
    pub importance_rank: Option<f64>,
    /// Dependencies on other features
    pub dependencies: Vec<String>,
    /// Computation cost (Low, Medium, High)
    pub computation_cost: ComputationCost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputationCost {
    Low,
    Medium,
    High,
}

/// Feature catalog - registry of all available features
pub struct FeatureCatalog {
    features: Vec<FeatureMetadata>,
}

impl FeatureCatalog {
    /// Create a new feature catalog with all 200+ features
    pub fn new() -> Self {
        let mut catalog = Self {
            features: Vec::with_capacity(250),
        };

        catalog.register_price_features();
        catalog.register_volume_features();
        catalog.register_liquidity_features();
        catalog.register_holder_features();
        catalog.register_technical_features();
        catalog.register_social_features();
        catalog.register_onchain_features();
        catalog.register_timeseries_features();
        catalog.register_interaction_features();
        catalog.register_market_features();

        catalog
    }

    /// Register price-based features (30+)
    fn register_price_features(&mut self) {
        let features = vec![
            ("price_current", "Current token price in SOL"),
            ("price_usd", "Current token price in USD"),
            ("price_change_1m", "Price change over 1 minute"),
            ("price_change_5m", "Price change over 5 minutes"),
            ("price_change_15m", "Price change over 15 minutes"),
            ("price_change_1h", "Price change over 1 hour"),
            ("price_change_24h", "Price change over 24 hours"),
            (
                "price_volatility_1h",
                "Price volatility (std dev) over 1 hour",
            ),
            ("price_volatility_24h", "Price volatility over 24 hours"),
            ("price_momentum_short", "Short-term price momentum"),
            ("price_momentum_medium", "Medium-term price momentum"),
            ("price_momentum_long", "Long-term price momentum"),
            ("price_acceleration", "Rate of change of momentum"),
            ("price_max_1h", "Maximum price in last hour"),
            ("price_min_1h", "Minimum price in last hour"),
            ("price_range_1h", "Price range in last hour"),
            ("price_ath", "All-time high price"),
            ("price_ath_distance", "Distance from ATH"),
            ("price_atl", "All-time low price"),
            ("price_atl_distance", "Distance from ATL"),
            ("price_mean_1h", "Mean price over 1 hour"),
            ("price_median_1h", "Median price over 1 hour"),
            ("price_percentile_95_1h", "95th percentile price 1 hour"),
            ("price_percentile_5_1h", "5th percentile price 1 hour"),
            ("price_trend_strength", "Strength of price trend"),
            ("price_reversal_indicator", "Potential reversal signal"),
            ("price_support_level", "Identified support level"),
            ("price_resistance_level", "Identified resistance level"),
            ("price_breakout_score", "Breakout probability score"),
            ("price_consolidation_score", "Consolidation pattern score"),
            ("price_divergence_score", "Price divergence from market"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::Price),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec![],
                computation_cost: ComputationCost::Low,
            });
        }
    }

    /// Register volume-based features (30+)
    fn register_volume_features(&mut self) {
        let features = vec![
            ("volume_1m", "Trading volume in last minute"),
            ("volume_5m", "Trading volume in last 5 minutes"),
            ("volume_15m", "Trading volume in last 15 minutes"),
            ("volume_1h", "Trading volume in last hour"),
            ("volume_24h", "Trading volume in last 24 hours"),
            ("volume_change_1m", "Volume change rate 1 minute"),
            ("volume_change_5m", "Volume change rate 5 minutes"),
            ("volume_change_1h", "Volume change rate 1 hour"),
            ("volume_acceleration", "Volume acceleration"),
            ("volume_momentum", "Volume momentum indicator"),
            ("volume_spike_score", "Volume spike detection"),
            ("volume_consistency", "Volume consistency metric"),
            ("volume_trend", "Volume trend direction"),
            ("volume_volatility", "Volume volatility measure"),
            ("volume_mean_1h", "Mean volume over 1 hour"),
            ("volume_std_1h", "Standard deviation of volume 1 hour"),
            ("volume_percentile_95", "95th percentile volume"),
            ("volume_percentile_5", "5th percentile volume"),
            ("buy_volume_1h", "Buy volume in last hour"),
            ("sell_volume_1h", "Sell volume in last hour"),
            ("buy_sell_ratio", "Ratio of buy to sell volume"),
            ("buy_sell_imbalance", "Buy-sell volume imbalance"),
            ("volume_weighted_price", "Volume weighted average price"),
            ("volume_distribution", "Volume distribution score"),
            ("volume_concentration", "Volume concentration metric"),
            ("large_trade_count", "Number of large trades"),
            ("small_trade_count", "Number of small trades"),
            ("trade_size_avg", "Average trade size"),
            ("trade_size_median", "Median trade size"),
            ("trade_frequency", "Trade frequency (trades/minute)"),
            ("volume_to_mcap_ratio", "Volume to market cap ratio"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::Volume),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec![],
                computation_cost: ComputationCost::Low,
            });
        }
    }

    /// Register liquidity features (20+)
    fn register_liquidity_features(&mut self) {
        let features = vec![
            ("liquidity_sol", "Total SOL liquidity in pools"),
            ("liquidity_token", "Total token liquidity in pools"),
            ("liquidity_usd", "Total USD liquidity value"),
            ("liquidity_change_1h", "Liquidity change over 1 hour"),
            ("liquidity_change_24h", "Liquidity change over 24 hours"),
            ("liquidity_depth_bids", "Bid side liquidity depth"),
            ("liquidity_depth_asks", "Ask side liquidity depth"),
            ("liquidity_imbalance", "Bid-ask liquidity imbalance"),
            ("slippage_1_sol", "Estimated slippage for 1 SOL trade"),
            ("slippage_10_sol", "Estimated slippage for 10 SOL trade"),
            ("slippage_100_sol", "Estimated slippage for 100 SOL trade"),
            ("pool_count", "Number of liquidity pools"),
            ("pool_concentration", "Liquidity concentration in pools"),
            ("price_impact_score", "Price impact score"),
            ("liquidity_velocity", "Rate of liquidity change"),
            ("liquidity_stability", "Liquidity stability metric"),
            ("liquidity_to_volume_ratio", "Liquidity to volume ratio"),
            ("effective_spread", "Effective bid-ask spread"),
            ("order_book_depth", "Order book depth score"),
            ("market_depth_score", "Market depth quality score"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::Liquidity),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec![],
                computation_cost: ComputationCost::Medium,
            });
        }
    }

    /// Register holder distribution features (30+)
    fn register_holder_features(&mut self) {
        let features = vec![
            ("holder_count", "Total number of holders"),
            ("holder_count_change_1h", "Holder count change 1 hour"),
            ("holder_count_change_24h", "Holder count change 24 hours"),
            ("holder_growth_rate", "Holder growth rate"),
            ("top_10_concentration", "Top 10 holders concentration"),
            ("top_50_concentration", "Top 50 holders concentration"),
            ("top_100_concentration", "Top 100 holders concentration"),
            ("gini_coefficient", "Gini coefficient of distribution"),
            ("holder_distribution_score", "Distribution quality score"),
            ("whale_count", "Number of whale holders"),
            ("whale_holdings_pct", "Percentage held by whales"),
            ("whale_activity_score", "Whale trading activity"),
            ("creator_holdings_pct", "Creator holdings percentage"),
            ("creator_sell_rate", "Creator selling rate"),
            ("creator_buy_back_rate", "Creator buy-back rate"),
            ("new_holders_1h", "New holders in last hour"),
            ("new_holders_24h", "New holders in last 24 hours"),
            ("holder_churn_rate", "Holder churn rate"),
            ("holder_retention_rate", "Holder retention rate"),
            ("avg_holder_balance", "Average holder balance"),
            ("median_holder_balance", "Median holder balance"),
            ("holder_balance_std", "Std dev of holder balances"),
            ("active_holders_pct", "Percentage of active holders"),
            ("holder_engagement_score", "Holder engagement metric"),
            ("holder_diversity_score", "Holder diversity metric"),
            ("holder_commitment_score", "Long-term holder commitment"),
            ("holder_turnover_rate", "Holder turnover rate"),
            ("holder_accumulation_score", "Holder accumulation pattern"),
            ("holder_distribution_entropy", "Entropy of distribution"),
            ("holder_power_law_exp", "Power law exponent of distribution"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::Holders),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec![],
                computation_cost: ComputationCost::Medium,
            });
        }
    }

    /// Register technical indicator features (30+)
    fn register_technical_features(&mut self) {
        let features = vec![
            ("rsi_14", "RSI with 14-period"),
            ("rsi_7", "RSI with 7-period"),
            ("rsi_divergence", "RSI divergence indicator"),
            ("macd", "MACD indicator"),
            ("macd_signal", "MACD signal line"),
            ("macd_histogram", "MACD histogram"),
            ("ema_12", "12-period EMA"),
            ("ema_26", "26-period EMA"),
            ("ema_50", "50-period EMA"),
            ("ema_200", "200-period EMA"),
            ("sma_20", "20-period SMA"),
            ("sma_50", "50-period SMA"),
            ("bollinger_upper", "Bollinger upper band"),
            ("bollinger_lower", "Bollinger lower band"),
            ("bollinger_width", "Bollinger band width"),
            ("bollinger_position", "Price position in Bollinger bands"),
            ("atr", "Average True Range"),
            ("atr_percent", "ATR as percentage of price"),
            ("stochastic_k", "Stochastic %K"),
            ("stochastic_d", "Stochastic %D"),
            ("stochastic_divergence", "Stochastic divergence"),
            ("adx", "Average Directional Index"),
            ("adx_trend_strength", "ADX trend strength"),
            ("cci", "Commodity Channel Index"),
            ("momentum_oscillator", "Momentum oscillator"),
            ("rate_of_change", "Rate of change indicator"),
            ("williams_r", "Williams %R"),
            ("ichimoku_cloud", "Ichimoku cloud position"),
            ("parabolic_sar", "Parabolic SAR"),
            ("pivot_points", "Pivot point levels"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::Technical),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec!["price_history".to_string()],
                computation_cost: ComputationCost::Medium,
            });
        }
    }

    /// Register social and sentiment features (20+)
    fn register_social_features(&mut self) {
        let features = vec![
            ("twitter_mentions", "Twitter mentions count"),
            ("twitter_mentions_growth", "Twitter mentions growth rate"),
            ("twitter_sentiment", "Twitter sentiment score"),
            ("twitter_engagement", "Twitter engagement rate"),
            ("telegram_members", "Telegram member count"),
            ("telegram_growth", "Telegram growth rate"),
            ("telegram_activity", "Telegram activity score"),
            ("discord_members", "Discord member count"),
            ("discord_activity", "Discord activity level"),
            ("reddit_mentions", "Reddit mentions count"),
            ("reddit_sentiment", "Reddit sentiment score"),
            ("social_velocity", "Social media velocity"),
            ("social_momentum", "Social momentum indicator"),
            ("influencer_mentions", "Influencer mention count"),
            ("social_engagement_rate", "Overall social engagement"),
            ("sentiment_score", "Aggregated sentiment score"),
            ("sentiment_volatility", "Sentiment volatility"),
            ("social_reach", "Estimated social reach"),
            ("community_strength", "Community strength metric"),
            ("social_trend", "Social trend indicator"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::Social),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec![],
                computation_cost: ComputationCost::High,
            });
        }
    }

    /// Register on-chain metrics (20+)
    fn register_onchain_features(&mut self) {
        let features = vec![
            ("tx_count_1h", "Transaction count last hour"),
            ("tx_count_24h", "Transaction count last 24 hours"),
            ("tx_count_growth", "Transaction count growth rate"),
            ("unique_wallets_1h", "Unique wallets last hour"),
            ("unique_wallets_24h", "Unique wallets last 24 hours"),
            ("wallet_growth_rate", "Wallet growth rate"),
            ("failed_tx_rate", "Failed transaction rate"),
            ("avg_tx_size", "Average transaction size"),
            ("median_tx_size", "Median transaction size"),
            ("large_tx_count", "Large transaction count"),
            ("smart_money_flow", "Smart money flow indicator"),
            ("dex_aggregator_usage", "DEX aggregator usage rate"),
            ("jito_bundle_rate", "Jito bundle usage rate"),
            ("mev_activity_score", "MEV activity score"),
            ("flash_loan_activity", "Flash loan activity"),
            ("contract_interaction_count", "Smart contract interactions"),
            ("program_call_diversity", "Program call diversity"),
            ("wallet_age_avg", "Average wallet age"),
            ("wallet_activity_score", "Wallet activity score"),
            ("on_chain_volume_velocity", "On-chain volume velocity"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::OnChain),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec![],
                computation_cost: ComputationCost::High,
            });
        }
    }

    /// Register time-series derived features (20+)
    fn register_timeseries_features(&mut self) {
        let features = vec![
            ("price_autocorr_1", "Price autocorrelation lag 1"),
            ("price_autocorr_5", "Price autocorrelation lag 5"),
            ("volume_autocorr_1", "Volume autocorrelation lag 1"),
            ("volume_autocorr_5", "Volume autocorrelation lag 5"),
            ("price_seasonality", "Price seasonality pattern"),
            ("volume_seasonality", "Volume seasonality pattern"),
            ("trend_strength", "Time series trend strength"),
            ("cyclical_component", "Cyclical component strength"),
            ("noise_level", "Noise level in price data"),
            ("hurst_exponent", "Hurst exponent (persistence)"),
            ("fractal_dimension", "Fractal dimension"),
            ("entropy_rate", "Entropy rate of price series"),
            ("lyapunov_exponent", "Lyapunov exponent (chaos)"),
            ("detrended_fluctuation", "Detrended fluctuation analysis"),
            ("spectral_density_peak", "Spectral density peak frequency"),
            ("wavelet_energy", "Wavelet energy distribution"),
            ("recurrence_rate", "Recurrence rate"),
            ("lag_correlation", "Cross-correlation lag"),
            ("granger_causality", "Granger causality score"),
            ("cointegration_score", "Cointegration with market"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::TimeSeries),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec!["price_history".to_string(), "volume_history".to_string()],
                computation_cost: ComputationCost::High,
            });
        }
    }

    /// Register interaction features (cross-feature combinations) (20+)
    fn register_interaction_features(&mut self) {
        let features = vec![
            ("price_volume_correlation", "Price-volume correlation"),
            ("liquidity_volume_ratio", "Liquidity to volume ratio"),
            ("holder_price_correlation", "Holder count-price correlation"),
            ("social_price_lag", "Social mentions leading price"),
            (
                "volume_volatility_product",
                "Volume × volatility interaction",
            ),
            (
                "liquidity_concentration_product",
                "Liquidity × concentration",
            ),
            ("sentiment_momentum_product", "Sentiment × momentum"),
            ("whale_price_impact", "Whale activity price impact"),
            (
                "creator_market_influence",
                "Creator activity market influence",
            ),
            ("social_volume_synergy", "Social activity-volume synergy"),
            (
                "technical_fundamental_align",
                "Technical-fundamental alignment",
            ),
            (
                "onchain_offchain_correlation",
                "On-chain/off-chain correlation",
            ),
            ("price_liquidity_efficiency", "Price discovery efficiency"),
            ("holder_social_engagement", "Holder-social engagement"),
            ("volume_distribution_quality", "Volume distribution quality"),
            (
                "momentum_sentiment_convergence",
                "Momentum-sentiment convergence",
            ),
            (
                "volatility_liquidity_stability",
                "Volatility-liquidity stability",
            ),
            ("growth_sustainability", "Growth sustainability index"),
            (
                "market_microstructure_quality",
                "Market microstructure quality",
            ),
            ("composite_health_score", "Composite token health score"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::Interaction),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec!["multiple".to_string()],
                computation_cost: ComputationCost::Medium,
            });
        }
    }

    /// Register market context features (10+)
    fn register_market_features(&mut self) {
        let features = vec![
            ("market_regime", "Current market regime"),
            ("sol_price_usd", "SOL/USD price"),
            ("sol_volatility", "SOL volatility"),
            ("market_sentiment", "Overall market sentiment"),
            ("market_volume_24h", "Total market volume 24h"),
            ("new_token_launch_rate", "New token launch rate"),
            ("market_liquidity", "Overall market liquidity"),
            ("network_congestion", "Network congestion level"),
            ("gas_price_level", "Gas price level"),
            ("market_correlation", "Correlation with market"),
        ];

        for (name, desc) in features {
            self.features.push(FeatureMetadata {
                id: FeatureId::new(name, FeatureCategory::Market),
                description: desc.to_string(),
                value_range: (0.0, 1.0),
                requires_normalization: true,
                importance_rank: None,
                dependencies: vec![],
                computation_cost: ComputationCost::Low,
            });
        }
    }

    /// Get all features in the catalog
    pub fn all_features(&self) -> &[FeatureMetadata] {
        &self.features
    }

    /// Get features by category
    pub fn features_by_category(&self, category: FeatureCategory) -> Vec<&FeatureMetadata> {
        self.features
            .iter()
            .filter(|f| f.id.category == category)
            .collect()
    }

    /// Get feature by name
    pub fn get_feature(&self, name: &str) -> Option<&FeatureMetadata> {
        self.features.iter().find(|f| f.id.name == name)
    }

    /// Get total feature count
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    /// Get feature count by category
    pub fn feature_count_by_category(&self, category: FeatureCategory) -> usize {
        self.features
            .iter()
            .filter(|f| f.id.category == category)
            .count()
    }
}

impl Default for FeatureCatalog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_creation() {
        let catalog = FeatureCatalog::new();
        assert!(
            catalog.feature_count() >= 200,
            "Should have at least 200 features"
        );
    }

    #[test]
    fn test_catalog_categories() {
        let catalog = FeatureCatalog::new();

        // Verify each category has features
        assert!(catalog.feature_count_by_category(FeatureCategory::Price) >= 30);
        assert!(catalog.feature_count_by_category(FeatureCategory::Volume) >= 30);
        assert!(catalog.feature_count_by_category(FeatureCategory::Liquidity) >= 20);
        assert!(catalog.feature_count_by_category(FeatureCategory::Holders) >= 30);
        assert!(catalog.feature_count_by_category(FeatureCategory::Technical) >= 30);
        assert!(catalog.feature_count_by_category(FeatureCategory::Social) >= 20);
        assert!(catalog.feature_count_by_category(FeatureCategory::OnChain) >= 20);
        assert!(catalog.feature_count_by_category(FeatureCategory::TimeSeries) >= 20);
    }

    #[test]
    fn test_feature_lookup() {
        let catalog = FeatureCatalog::new();

        let feature = catalog.get_feature("price_current");
        assert!(feature.is_some());
        assert_eq!(feature.unwrap().id.category, FeatureCategory::Price);
    }

    #[test]
    fn test_features_by_category() {
        let catalog = FeatureCatalog::new();

        let price_features = catalog.features_by_category(FeatureCategory::Price);
        assert!(!price_features.is_empty());

        for feature in price_features {
            assert_eq!(feature.id.category, FeatureCategory::Price);
        }
    }
}
