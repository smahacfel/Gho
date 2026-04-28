pub mod catalog;
pub mod extractors;
pub mod graph_extractor;
pub mod normalization;
pub mod selection;
pub mod store;
pub mod worker;

pub use catalog::{FeatureCatalog, FeatureCategory, FeatureId, FeatureMetadata};
pub use extractors::{
    FeatureExtractionPipeline, FeatureExtractor, FeatureValue, FeatureVector,
    HolderFeatureExtractor, InteractionFeatureExtractor, LiquidityFeatureExtractor,
    OnChainFeatureExtractor, PriceFeatureExtractor, SocialFeatureExtractor, VolumeFeatureExtractor,
};
pub use graph_extractor::GraphAnalysisExtractor;
pub use normalization::{BatchNormalizer, FeatureNormalizer, NormalizationMethod};
pub use selection::{
    CorrelationSelector, FeatureImportance, FeatureSelector, MutualInformationSelector,
    SelectionMethod, VarianceSelector,
};
pub use store::{BatchFeatureStore, FeatureEntry, FeatureStore, PersistentFeatureStore, StoreKey};
pub use worker::{FeatureWorker, FeatureWorkerConfig, FeatureWorkerHandle};

use crate::oracle::types_old::TokenData;
use crate::types::PremintCandidate;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, instrument};

/// Complete feature engineering pipeline
pub struct FeatureEngineeringPipeline {
    /// Feature catalog
    catalog: FeatureCatalog,
    /// Feature extraction pipeline
    extractor: FeatureExtractionPipeline,
    /// Feature normalizer
    normalizer: FeatureNormalizer,
    /// Feature selector
    selector: Option<FeatureSelector>,
    /// Feature store (cache)
    store: Arc<FeatureStore>,
}

impl FeatureEngineeringPipeline {
    /// Create a new feature engineering pipeline
    pub fn new() -> Self {
        Self {
            catalog: FeatureCatalog::new(),
            extractor: FeatureExtractionPipeline::new(),
            normalizer: FeatureNormalizer::new(NormalizationMethod::MinMax),
            selector: None,
            store: Arc::new(FeatureStore::new(1000, Duration::from_secs(300))),
        }
    }

    /// Create a pipeline with custom configuration
    pub fn with_config(
        normalization_method: NormalizationMethod,
        cache_size: u64,
        cache_ttl: Duration,
    ) -> Self {
        Self {
            catalog: FeatureCatalog::new(),
            extractor: FeatureExtractionPipeline::new(),
            normalizer: FeatureNormalizer::new(normalization_method),
            selector: None,
            store: Arc::new(FeatureStore::new(cache_size, cache_ttl)),
        }
    }

    /// Get the feature catalog
    pub fn catalog(&self) -> &FeatureCatalog {
        &self.catalog
    }

    /// Get the feature store
    pub fn store(&self) -> Arc<FeatureStore> {
        Arc::clone(&self.store)
    }

    /// Set feature selector for automatic feature selection
    pub fn set_selector(&mut self, selector: FeatureSelector) {
        self.selector = Some(selector);
    }

    /// Extract raw features from token data
    #[instrument(skip(self, candidate, token_data), fields(mint = %candidate.mint))]
    pub async fn extract_features(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let key = candidate.mint.to_string();

        // Try to get from cache first
        if let Some(cached) = self.store.get(&key).await {
            debug!("Using cached features for {}", key);
            return Ok(cached);
        }

        // Extract features
        let features = self.extractor.extract_all(candidate, token_data)?;

        // Store in cache
        self.store.store(key, features.clone()).await?;

        Ok(features)
    }

    /// Extract and normalize features
    pub async fn extract_and_normalize(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let features = self.extract_features(candidate, token_data).await?;
        self.normalizer.normalize(&features)
    }

    /// Extract, normalize, and select features
    pub async fn extract_processed(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData,
        selection_method: Option<SelectionMethod>,
    ) -> Result<FeatureVector> {
        let features = self.extract_and_normalize(candidate, token_data).await?;

        // Apply feature selection if configured
        if let (Some(selector), Some(method)) = (&self.selector, selection_method) {
            Ok(selector.filter_features(&features, method))
        } else {
            Ok(features)
        }
    }

    /// Fit normalizer on training data
    pub fn fit_normalizer(&mut self, training_data: &[FeatureVector]) -> Result<()> {
        self.normalizer.fit(training_data)?;
        debug!("Fitted normalizer on {} samples", training_data.len());
        Ok(())
    }

    /// Train feature selector on labeled data
    pub fn train_selector(
        &mut self,
        feature_vectors: &[FeatureVector],
        targets: &[f64],
    ) -> Result<()> {
        let importance = MutualInformationSelector::rank_features(feature_vectors, targets)?;
        self.selector = Some(FeatureSelector::new(importance));
        debug!(
            "Trained feature selector on {} samples",
            feature_vectors.len()
        );
        Ok(())
    }

    /// Get summary statistics
    pub async fn get_stats(&self) -> PipelineStats {
        let cache_stats = self.store.stats().await;

        PipelineStats {
            total_features: self.catalog.feature_count(),
            available_extractors: self.extractor.available_features().len(),
            cache_entries: cache_stats.entry_count,
            has_selector: self.selector.is_some(),
        }
    }

    /// Clear feature cache
    pub async fn clear_cache(&self) {
        self.store.clear().await;
        debug!("Cleared feature cache");
    }
}

impl Default for FeatureEngineeringPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Pipeline statistics
#[derive(Debug, Clone)]
pub struct PipelineStats {
    pub total_features: usize,
    pub available_extractors: usize,
    pub cache_entries: u64,
    pub has_selector: bool,
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

    #[tokio::test]
    async fn test_pipeline_creation() {
        let pipeline = FeatureEngineeringPipeline::new();
        let stats = pipeline.get_stats().await;

        assert!(stats.total_features >= 200);
        assert!(stats.available_extractors > 0);
    }

    #[tokio::test]
    async fn test_feature_extraction() {
        let pipeline = FeatureEngineeringPipeline::new();
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        let features = pipeline
            .extract_features(&candidate, &token_data)
            .await
            .unwrap();

        assert!(!features.is_empty());
        assert!(features.contains_key("price_current"));
        assert!(features.contains_key("volume_24h"));
    }

    #[tokio::test]
    async fn test_feature_normalization() {
        let pipeline = FeatureEngineeringPipeline::new();
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        let features = pipeline
            .extract_and_normalize(&candidate, &token_data)
            .await
            .unwrap();

        // All normalized features should be valid numbers
        for (_name, value) in features {
            assert!(value.is_finite());
        }
    }

    #[tokio::test]
    async fn test_feature_caching() {
        let pipeline = FeatureEngineeringPipeline::new();
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        // First extraction
        let features1 = pipeline
            .extract_features(&candidate, &token_data)
            .await
            .unwrap();

        // Second extraction should hit cache
        let features2 = pipeline
            .extract_features(&candidate, &token_data)
            .await
            .unwrap();

        assert_eq!(features1.len(), features2.len());
    }

    #[tokio::test]
    async fn test_catalog_access() {
        let pipeline = FeatureEngineeringPipeline::new();
        let catalog = pipeline.catalog();

        assert!(catalog.feature_count() >= 200);

        let price_features = catalog.features_by_category(FeatureCategory::Price);
        assert!(!price_features.is_empty());
    }

    #[test]
    fn test_custom_configuration() {
        let pipeline = FeatureEngineeringPipeline::with_config(
            NormalizationMethod::ZScore,
            500,
            Duration::from_secs(600),
        );

        assert!(pipeline.catalog().feature_count() >= 200);
    }
}
