//! Feature Extractors - Logic for computing feature values
//!
//! This module contains the implementation of feature extraction logic
//! for all features defined in the catalog.

use super::catalog::{FeatureCategory, FeatureId};
use crate::oracle::types_old::TokenData;
use crate::types::PremintCandidate;
use anyhow::Result;
use std::collections::HashMap;
use tracing::{debug, instrument};

/// Feature value storage
pub type FeatureValue = f64;
pub type FeatureVector = HashMap<String, FeatureValue>;

/// Feature extractor trait for extensibility
pub trait FeatureExtractor: Send + Sync {
    /// Extract features from token data
    fn extract(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector>;

    /// Get the category this extractor handles
    fn category(&self) -> FeatureCategory;

    /// Get list of feature names this extractor produces
    fn feature_names(&self) -> Vec<String>;
}

/// Price feature extractor
pub struct PriceFeatureExtractor;

impl FeatureExtractor for PriceFeatureExtractor {
    fn extract(
        &self,
        _candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let mut features = HashMap::new();

        // Current price
        if let Some(&current_price) = token_data.price_history.back() {
            features.insert("price_current".to_string(), current_price);

            // Price changes
            if let Some(&initial_price) = token_data.price_history.front() {
                let price_change = (current_price - initial_price) / initial_price.max(0.0001);
                features.insert("price_change_24h".to_string(), price_change);

                // Calculate momentum
                let momentum = if price_change > 0.0 {
                    price_change.sqrt()
                } else {
                    -(-price_change).sqrt()
                };
                features.insert("price_momentum_short".to_string(), momentum);
            }

            // Volatility (simplified)
            let prices: Vec<f64> = token_data.price_history.iter().copied().collect();
            if prices.len() >= 2 {
                let mean = prices.iter().sum::<f64>() / prices.len() as f64;
                let variance =
                    prices.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / prices.len() as f64;
                let volatility = variance.sqrt() / mean;
                features.insert("price_volatility_1h".to_string(), volatility);
            }

            // Price range
            if let (Some(&max), Some(&min)) = (
                prices.iter().max_by(|a, b| a.partial_cmp(b).unwrap()),
                prices.iter().min_by(|a, b| a.partial_cmp(b).unwrap()),
            ) {
                features.insert("price_max_1h".to_string(), max);
                features.insert("price_min_1h".to_string(), min);
                features.insert("price_range_1h".to_string(), max - min);
            }
        }

        Ok(features)
    }

    fn category(&self) -> FeatureCategory {
        FeatureCategory::Price
    }

    fn feature_names(&self) -> Vec<String> {
        vec![
            "price_current".to_string(),
            "price_change_24h".to_string(),
            "price_momentum_short".to_string(),
            "price_volatility_1h".to_string(),
            "price_max_1h".to_string(),
            "price_min_1h".to_string(),
            "price_range_1h".to_string(),
        ]
    }
}

/// Volume feature extractor
pub struct VolumeFeatureExtractor;

impl FeatureExtractor for VolumeFeatureExtractor {
    fn extract(
        &self,
        _candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let mut features = HashMap::new();

        let volume_data = &token_data.volume_data;

        // Current volume
        features.insert("volume_24h".to_string(), volume_data.current_volume);

        // Volume growth
        features.insert(
            "volume_change_1h".to_string(),
            volume_data.volume_growth_rate,
        );

        // Buy/sell ratio
        features.insert("buy_sell_ratio".to_string(), volume_data.buy_sell_ratio);

        // Transaction count
        features.insert(
            "trade_frequency".to_string(),
            volume_data.transaction_count as f64,
        );

        // Volume momentum (simplified)
        let volume_momentum = if volume_data.volume_growth_rate > 1.0 {
            (volume_data.volume_growth_rate - 1.0).min(10.0)
        } else {
            0.0
        };
        features.insert("volume_momentum".to_string(), volume_momentum);

        // Volume spike detection
        let spike_score = if volume_data.volume_growth_rate > 5.0 {
            1.0
        } else {
            0.0
        };
        features.insert("volume_spike_score".to_string(), spike_score);

        Ok(features)
    }

    fn category(&self) -> FeatureCategory {
        FeatureCategory::Volume
    }

    fn feature_names(&self) -> Vec<String> {
        vec![
            "volume_24h".to_string(),
            "volume_change_1h".to_string(),
            "buy_sell_ratio".to_string(),
            "trade_frequency".to_string(),
            "volume_momentum".to_string(),
            "volume_spike_score".to_string(),
        ]
    }
}

/// Liquidity feature extractor
pub struct LiquidityFeatureExtractor;

impl FeatureExtractor for LiquidityFeatureExtractor {
    fn extract(
        &self,
        _candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let mut features = HashMap::new();

        if let Some(pool) = &token_data.liquidity_pool {
            features.insert("liquidity_sol".to_string(), pool.sol_amount);
            features.insert("liquidity_token".to_string(), pool.token_amount);

            // Liquidity to volume ratio
            let liq_vol_ratio = pool.sol_amount / token_data.volume_data.current_volume.max(1.0);
            features.insert("liquidity_to_volume_ratio".to_string(), liq_vol_ratio);

            // Estimated slippage (simplified)
            let slippage_1sol = 1.0 / pool.sol_amount.max(1.0);
            features.insert("slippage_1_sol".to_string(), slippage_1sol);

            let slippage_10sol = 10.0 / pool.sol_amount.max(1.0);
            features.insert("slippage_10_sol".to_string(), slippage_10sol);
        } else {
            features.insert("liquidity_sol".to_string(), 0.0);
            features.insert("liquidity_token".to_string(), 0.0);
            features.insert("liquidity_to_volume_ratio".to_string(), 0.0);
            features.insert("slippage_1_sol".to_string(), 1.0);
            features.insert("slippage_10_sol".to_string(), 1.0);
        }

        Ok(features)
    }

    fn category(&self) -> FeatureCategory {
        FeatureCategory::Liquidity
    }

    fn feature_names(&self) -> Vec<String> {
        vec![
            "liquidity_sol".to_string(),
            "liquidity_token".to_string(),
            "liquidity_to_volume_ratio".to_string(),
            "slippage_1_sol".to_string(),
            "slippage_10_sol".to_string(),
        ]
    }
}

/// Holder distribution feature extractor
pub struct HolderFeatureExtractor;

impl FeatureExtractor for HolderFeatureExtractor {
    fn extract(
        &self,
        _candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let mut features = HashMap::new();

        // Holder count
        features.insert(
            "holder_count".to_string(),
            token_data.holder_distribution.len() as f64,
        );

        // Top holder concentration
        let top_10_concentration: f64 = token_data
            .holder_distribution
            .iter()
            .take(10)
            .map(|h| h.percentage)
            .sum();
        features.insert("top_10_concentration".to_string(), top_10_concentration);

        let top_50_concentration: f64 = token_data
            .holder_distribution
            .iter()
            .take(50)
            .map(|h| h.percentage)
            .sum();
        features.insert("top_50_concentration".to_string(), top_50_concentration);

        // Distribution quality score (inverse of concentration)
        let distribution_score = 1.0 - top_10_concentration.min(1.0);
        features.insert("holder_distribution_score".to_string(), distribution_score);

        // Whale count
        let whale_count = token_data
            .holder_distribution
            .iter()
            .filter(|h| h.is_whale)
            .count();
        features.insert("whale_count".to_string(), whale_count as f64);

        // Holder growth
        if token_data.holder_history.len() >= 2 {
            let current = *token_data.holder_history.back().unwrap_or(&0) as f64;
            let initial = *token_data.holder_history.front().unwrap_or(&1) as f64;
            let growth_rate = (current - initial) / initial.max(1.0);
            features.insert("holder_growth_rate".to_string(), growth_rate);
        }

        // Creator holdings
        let creator_pct =
            token_data.creator_holdings.current_balance as f64 / token_data.supply as f64;
        features.insert("creator_holdings_pct".to_string(), creator_pct);

        Ok(features)
    }

    fn category(&self) -> FeatureCategory {
        FeatureCategory::Holders
    }

    fn feature_names(&self) -> Vec<String> {
        vec![
            "holder_count".to_string(),
            "top_10_concentration".to_string(),
            "top_50_concentration".to_string(),
            "holder_distribution_score".to_string(),
            "whale_count".to_string(),
            "holder_growth_rate".to_string(),
            "creator_holdings_pct".to_string(),
        ]
    }
}

/// Social activity feature extractor
pub struct SocialFeatureExtractor;

impl FeatureExtractor for SocialFeatureExtractor {
    fn extract(
        &self,
        _candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let mut features = HashMap::new();

        let social = &token_data.social_activity;

        features.insert(
            "twitter_mentions".to_string(),
            social.twitter_mentions as f64,
        );
        features.insert(
            "telegram_members".to_string(),
            social.telegram_members as f64,
        );
        features.insert("discord_members".to_string(), social.discord_members as f64);
        features.insert("sentiment_score".to_string(), social.social_score);

        // Social engagement (combined metric)
        let total_engagement = (social.twitter_mentions as f64 * 0.4
            + social.telegram_members as f64 * 0.3
            + social.discord_members as f64 * 0.3)
            / 1000.0;
        features.insert(
            "social_engagement_rate".to_string(),
            total_engagement.min(1.0),
        );

        Ok(features)
    }

    fn category(&self) -> FeatureCategory {
        FeatureCategory::Social
    }

    fn feature_names(&self) -> Vec<String> {
        vec![
            "twitter_mentions".to_string(),
            "telegram_members".to_string(),
            "discord_members".to_string(),
            "sentiment_score".to_string(),
            "social_engagement_rate".to_string(),
        ]
    }
}

/// On-chain metrics feature extractor
pub struct OnChainFeatureExtractor;

impl FeatureExtractor for OnChainFeatureExtractor {
    fn extract(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let mut features = HashMap::new();

        // Transaction metrics from volume data
        features.insert(
            "tx_count_24h".to_string(),
            token_data.volume_data.transaction_count as f64,
        );

        // Jito bundle presence
        let jito_score = match candidate.is_jito_bundle {
            Some(true) => 1.0,
            Some(false) => 0.0,
            None => 0.5,
        };
        features.insert("jito_bundle_rate".to_string(), jito_score);

        // Estimated unique wallets (simplified: assume each holder is unique)
        features.insert(
            "unique_wallets_24h".to_string(),
            token_data.holder_distribution.len() as f64,
        );

        Ok(features)
    }

    fn category(&self) -> FeatureCategory {
        FeatureCategory::OnChain
    }

    fn feature_names(&self) -> Vec<String> {
        vec![
            "tx_count_24h".to_string(),
            "jito_bundle_rate".to_string(),
            "unique_wallets_24h".to_string(),
        ]
    }
}

/// Interaction feature extractor (cross-feature combinations)
pub struct InteractionFeatureExtractor;

impl FeatureExtractor for InteractionFeatureExtractor {
    fn extract(
        &self,
        _candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let mut features = HashMap::new();

        // Price-volume correlation (simplified)
        if token_data.price_history.len() >= 2 {
            let price_trend = if let (Some(&first), Some(&last)) = (
                token_data.price_history.front(),
                token_data.price_history.back(),
            ) {
                (last - first) / first.max(0.0001)
            } else {
                0.0
            };

            let volume_trend = token_data.volume_data.volume_growth_rate - 1.0;

            // Positive correlation: both increase together
            let correlation = if price_trend * volume_trend > 0.0 {
                1.0
            } else {
                -1.0
            };
            features.insert(
                "price_volume_correlation".to_string(),
                (correlation + 1.0) / 2.0,
            );
        }

        // Liquidity-volume efficiency
        if let Some(pool) = &token_data.liquidity_pool {
            let efficiency = pool.sol_amount / token_data.volume_data.current_volume.max(1.0);
            features.insert(
                "liquidity_volume_ratio".to_string(),
                efficiency.min(10.0) / 10.0,
            );
        }

        // Holder-price health
        let holder_count = token_data.holder_distribution.len() as f64;
        let top_concentration: f64 = token_data
            .holder_distribution
            .iter()
            .take(10)
            .map(|h| h.percentage)
            .sum();
        let health = (holder_count / 100.0).min(1.0) * (1.0 - top_concentration);
        features.insert("composite_health_score".to_string(), health);

        Ok(features)
    }

    fn category(&self) -> FeatureCategory {
        FeatureCategory::Interaction
    }

    fn feature_names(&self) -> Vec<String> {
        vec![
            "price_volume_correlation".to_string(),
            "liquidity_volume_ratio".to_string(),
            "composite_health_score".to_string(),
        ]
    }
}

/// Main feature extraction coordinator
pub struct FeatureExtractionPipeline {
    extractors: Vec<Box<dyn FeatureExtractor>>,
}

impl FeatureExtractionPipeline {
    /// Create a new feature extraction pipeline with all extractors
    pub fn new() -> Self {
        let extractors: Vec<Box<dyn FeatureExtractor>> = vec![
            Box::new(PriceFeatureExtractor),
            Box::new(VolumeFeatureExtractor),
            Box::new(LiquidityFeatureExtractor),
            Box::new(HolderFeatureExtractor),
            Box::new(SocialFeatureExtractor),
            Box::new(OnChainFeatureExtractor),
            Box::new(InteractionFeatureExtractor),
        ];

        Self { extractors }
    }

    /// Extract all features from token data
    #[instrument(skip(self, candidate, token_data), fields(mint = %candidate.mint))]
    pub fn extract_all(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let mut all_features = HashMap::new();

        for extractor in &self.extractors {
            match extractor.extract(candidate, token_data) {
                Ok(features) => {
                    all_features.extend(features);
                }
                Err(e) => {
                    debug!(
                        "Error extracting {:?} features: {}",
                        extractor.category(),
                        e
                    );
                }
            }
        }

        debug!("Extracted {} features", all_features.len());
        Ok(all_features)
    }

    /// Extract features from specific categories
    pub fn extract_categories(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData,
        categories: &[FeatureCategory],
    ) -> Result<FeatureVector> {
        let mut features = HashMap::new();

        for extractor in &self.extractors {
            if categories.contains(&extractor.category()) {
                features.extend(extractor.extract(candidate, token_data)?);
            }
        }

        Ok(features)
    }

    /// Get all available feature names
    pub fn available_features(&self) -> Vec<String> {
        self.extractors
            .iter()
            .flat_map(|e| e.feature_names())
            .collect()
    }
}

impl Default for FeatureExtractionPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types_old::*;
    use crate::types::Pubkey;
    use std::collections::VecDeque;

    fn create_test_candidate() -> PremintCandidate {
        PremintCandidate {
            mint: "TestMint123456789".to_string(),
            creator: "TestCreator123456789".to_string(),
            program: "test".to_string(),
            slot: 12345,
            timestamp: 1640995200,
            instruction_summary: None,
            is_jito_bundle: Some(true),
        }
    }

    fn create_test_token_data() -> TokenData {
        TokenData {
            supply: 1_000_000_000,
            decimals: 9,
            metadata_uri: "https://example.com/metadata.json".to_string(),
            metadata: Some(Metadata {
                name: "Test Token".to_string(),
                symbol: "TEST".to_string(),
                description: "A test token".to_string(),
                image: "https://example.com/image.png".to_string(),
                attributes: vec![],
            }),
            holder_distribution: vec![HolderData {
                address: "Holder123456789".to_string(),
                percentage: 0.1,
                is_whale: false,
            }],
            liquidity_pool: Some(LiquidityPool {
                sol_amount: 50.0,
                token_amount: 1000.0,
                pool_address: "Pool123456789".to_string(),
                pool_type: PoolType::PumpFun,
            }),
            volume_data: VolumeData {
                initial_volume: 100.0,
                current_volume: 300.0,
                volume_growth_rate: 3.0,
                transaction_count: 50,
                buy_sell_ratio: 1.5,
            },
            creator_holdings: CreatorHoldings {
                initial_balance: 100_000_000,
                current_balance: 90_000_000,
                first_sell_timestamp: Some(1640995500),
                sell_transactions: 2,
            },
            holder_history: {
                let mut hist = VecDeque::new();
                hist.push_back(10);
                hist.push_back(25);
                hist
            },
            price_history: {
                let mut hist = VecDeque::new();
                hist.push_back(0.001);
                hist.push_back(0.0015);
                hist
            },
            social_activity: SocialActivity {
                twitter_mentions: 50,
                telegram_members: 200,
                discord_members: 100,
                social_score: 0.7,
            },
        }
    }

    #[test]
    fn test_price_extractor() {
        let extractor = PriceFeatureExtractor;
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        let features = extractor.extract(&candidate, &token_data).unwrap();

        assert!(features.contains_key("price_current"));
        assert!(features.contains_key("price_change_24h"));
        assert!(features.len() > 0);
    }

    #[test]
    fn test_volume_extractor() {
        let extractor = VolumeFeatureExtractor;
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        let features = extractor.extract(&candidate, &token_data).unwrap();

        assert!(features.contains_key("volume_24h"));
        assert!(features.contains_key("buy_sell_ratio"));
    }

    #[test]
    fn test_liquidity_extractor() {
        let extractor = LiquidityFeatureExtractor;
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        let features = extractor.extract(&candidate, &token_data).unwrap();

        assert!(features.contains_key("liquidity_sol"));
        assert!(features.get("liquidity_sol").unwrap() > &0.0);
    }

    #[test]
    fn test_holder_extractor() {
        let extractor = HolderFeatureExtractor;
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        let features = extractor.extract(&candidate, &token_data).unwrap();

        assert!(features.contains_key("holder_count"));
        assert!(features.contains_key("top_10_concentration"));
    }

    #[test]
    fn test_pipeline_extraction() {
        let pipeline = FeatureExtractionPipeline::new();
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        let features = pipeline.extract_all(&candidate, &token_data).unwrap();

        // Should have features from multiple categories
        assert!(features.len() > 10);
        assert!(features.contains_key("price_current"));
        assert!(features.contains_key("volume_24h"));
        assert!(features.contains_key("liquidity_sol"));
    }

    #[test]
    fn test_category_filtering() {
        let pipeline = FeatureExtractionPipeline::new();
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        let categories = vec![FeatureCategory::Price, FeatureCategory::Volume];
        let features = pipeline
            .extract_categories(&candidate, &token_data, &categories)
            .unwrap();

        assert!(features.contains_key("price_current"));
        assert!(features.contains_key("volume_24h"));
    }
}
