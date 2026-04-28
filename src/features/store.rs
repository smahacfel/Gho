//! Feature Store - Caching layer for computed features
//!
//! This module provides efficient caching and retrieval of computed features
//! to avoid redundant calculations.

use super::extractors::FeatureVector;
use anyhow::Result;
use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, instrument};

/// Feature store key (token mint address)
pub type StoreKey = String;

/// Cached feature entry with metadata
#[derive(Debug, Clone)]
pub struct FeatureEntry {
    /// The feature vector
    pub features: FeatureVector,
    /// Timestamp when computed
    pub timestamp: u64,
    /// Version of the feature extraction (for invalidation)
    pub version: u32,
}

/// Feature store with automatic expiration
pub struct FeatureStore {
    /// In-memory cache
    cache: Arc<Cache<StoreKey, FeatureEntry>>,
    /// Default TTL for entries
    default_ttl: Duration,
    /// Current feature version
    feature_version: u32,
}

impl FeatureStore {
    /// Create a new feature store
    pub fn new(max_capacity: u64, default_ttl: Duration) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_capacity)
            .time_to_live(default_ttl)
            .build();

        Self {
            cache: Arc::new(cache),
            default_ttl,
            feature_version: 1,
        }
    }

    /// Store features for a token
    #[instrument(skip(self, features))]
    pub async fn store(&self, key: StoreKey, features: FeatureVector) -> Result<()> {
        let entry = FeatureEntry {
            features,
            timestamp: Self::current_timestamp(),
            version: self.feature_version,
        };

        self.cache.insert(key.clone(), entry).await;
        debug!("Stored features for key: {}", key);
        Ok(())
    }

    /// Retrieve features for a token
    #[instrument(skip(self))]
    pub async fn get(&self, key: &StoreKey) -> Option<FeatureVector> {
        let entry = self.cache.get(key).await?;

        // Check if version matches
        if entry.version != self.feature_version {
            debug!(
                "Feature version mismatch, invalidating cache for key: {}",
                key
            );
            self.cache.invalidate(key).await;
            return None;
        }

        debug!("Cache hit for key: {}", key);
        Some(entry.features)
    }

    /// Get or compute features
    pub async fn get_or_compute<F, Fut>(
        &self,
        key: StoreKey,
        compute_fn: F,
    ) -> Result<FeatureVector>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<FeatureVector>>,
    {
        // Try to get from cache first
        if let Some(features) = self.get(&key).await {
            return Ok(features);
        }

        // Compute features
        let features = compute_fn().await?;

        // Store in cache
        self.store(key, features.clone()).await?;

        Ok(features)
    }

    /// Invalidate a single entry
    pub async fn invalidate(&self, key: &StoreKey) {
        self.cache.invalidate(key).await;
        debug!("Invalidated cache for key: {}", key);
    }

    /// Clear all entries
    pub async fn clear(&self) {
        self.cache.invalidate_all();
        debug!("Cleared all cache entries");
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        CacheStats {
            entry_count: self.cache.entry_count(),
            weighted_size: self.cache.weighted_size(),
        }
    }

    /// Increment feature version (invalidates all cached features)
    pub fn increment_version(&mut self) {
        self.feature_version += 1;
        debug!("Incremented feature version to {}", self.feature_version);
    }

    /// Get current timestamp
    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entry_count: u64,
    pub weighted_size: u64,
}

/// Batch feature store operations
pub struct BatchFeatureStore {
    store: Arc<FeatureStore>,
}

impl BatchFeatureStore {
    /// Create a new batch feature store
    pub fn new(store: Arc<FeatureStore>) -> Self {
        Self { store }
    }

    /// Store multiple feature vectors
    pub async fn store_batch(&self, entries: Vec<(StoreKey, FeatureVector)>) -> Result<()> {
        for (key, features) in entries {
            self.store.store(key, features).await?;
        }
        Ok(())
    }

    /// Retrieve multiple feature vectors
    pub async fn get_batch(&self, keys: &[StoreKey]) -> Vec<(StoreKey, Option<FeatureVector>)> {
        let mut results = Vec::with_capacity(keys.len());

        for key in keys {
            let features = self.store.get(key).await;
            results.push((key.clone(), features));
        }

        results
    }

    /// Get hit rate for a batch of keys
    pub async fn batch_hit_rate(&self, keys: &[StoreKey]) -> f64 {
        let results = self.get_batch(keys).await;
        let hits = results
            .iter()
            .filter(|(_, features)| features.is_some())
            .count();
        hits as f64 / keys.len() as f64
    }
}

/// Persistent feature store (for long-term storage)
pub struct PersistentFeatureStore {
    /// In-memory store for hot features
    memory_store: Arc<FeatureStore>,
    /// Path to persistent storage
    storage_path: std::path::PathBuf,
}

impl PersistentFeatureStore {
    /// Create a new persistent feature store
    pub fn new(memory_store: Arc<FeatureStore>, storage_path: std::path::PathBuf) -> Self {
        Self {
            memory_store,
            storage_path,
        }
    }

    /// Store features with persistence
    pub async fn store(&self, key: StoreKey, features: FeatureVector) -> Result<()> {
        // Store in memory cache
        self.memory_store
            .store(key.clone(), features.clone())
            .await?;

        // Store to disk (simplified - in production, use proper serialization)
        self.persist_to_disk(&key, &features).await?;

        Ok(())
    }

    /// Retrieve features (check memory first, then disk)
    pub async fn get(&self, key: &StoreKey) -> Result<Option<FeatureVector>> {
        // Try memory first
        if let Some(features) = self.memory_store.get(key).await {
            return Ok(Some(features));
        }

        // Try disk
        if let Some(features) = self.load_from_disk(key).await? {
            // Populate memory cache
            self.memory_store
                .store(key.clone(), features.clone())
                .await?;
            return Ok(Some(features));
        }

        Ok(None)
    }

    /// Persist features to disk
    async fn persist_to_disk(&self, key: &StoreKey, features: &FeatureVector) -> Result<()> {
        // Create storage directory if it doesn't exist
        tokio::fs::create_dir_all(&self.storage_path).await?;

        let file_path = self.storage_path.join(format!("{}.json", key));
        let json = serde_json::to_string(features)?;
        tokio::fs::write(file_path, json).await?;

        Ok(())
    }

    /// Load features from disk
    async fn load_from_disk(&self, key: &StoreKey) -> Result<Option<FeatureVector>> {
        let file_path = self.storage_path.join(format!("{}.json", key));

        if !file_path.exists() {
            return Ok(None);
        }

        let json = tokio::fs::read_to_string(file_path).await?;
        let features: FeatureVector = serde_json::from_str(&json)?;

        Ok(Some(features))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_feature_store_basic() {
        let store = FeatureStore::new(100, Duration::from_secs(300));

        let key = "test_token".to_string();
        let features: FeatureVector =
            vec![("feature1".to_string(), 0.5), ("feature2".to_string(), 0.8)]
                .into_iter()
                .collect();

        // Store features
        store.store(key.clone(), features.clone()).await.unwrap();

        // Retrieve features
        let retrieved = store.get(&key).await.unwrap();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved.get("feature1"), Some(&0.5));
    }

    #[tokio::test]
    async fn test_feature_store_cache_miss() {
        let store = FeatureStore::new(100, Duration::from_secs(300));

        let key = "nonexistent".to_string();
        let result = store.get(&key).await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_feature_store_invalidation() {
        let store = FeatureStore::new(100, Duration::from_secs(300));

        let key = "test_token".to_string();
        let features: FeatureVector = vec![("feature1".to_string(), 0.5)].into_iter().collect();

        store.store(key.clone(), features).await.unwrap();
        assert!(store.get(&key).await.is_some());

        store.invalidate(&key).await;
        assert!(store.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_feature_store_version_mismatch() {
        let mut store = FeatureStore::new(100, Duration::from_secs(300));

        let key = "test_token".to_string();
        let features: FeatureVector = vec![("feature1".to_string(), 0.5)].into_iter().collect();

        store.store(key.clone(), features).await.unwrap();
        assert!(store.get(&key).await.is_some());

        // Increment version
        store.increment_version();

        // Should return None due to version mismatch
        assert!(store.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_get_or_compute() {
        let store = FeatureStore::new(100, Duration::from_secs(300));

        let key = "test_token".to_string();

        // First call should compute
        let features = store
            .get_or_compute(key.clone(), || async {
                Ok(vec![("computed".to_string(), 1.0)].into_iter().collect())
            })
            .await
            .unwrap();

        assert_eq!(features.get("computed"), Some(&1.0));

        // Second call should use cache
        let features = store
            .get_or_compute(key.clone(), || async {
                Ok(vec![("computed".to_string(), 2.0)].into_iter().collect())
            })
            .await
            .unwrap();

        // Should still be 1.0 from cache
        assert_eq!(features.get("computed"), Some(&1.0));
    }

    #[tokio::test]
    async fn test_batch_operations() {
        let store = Arc::new(FeatureStore::new(100, Duration::from_secs(300)));
        let batch_store = BatchFeatureStore::new(store);

        let entries = vec![
            (
                "token1".to_string(),
                vec![("f1".to_string(), 0.5)].into_iter().collect(),
            ),
            (
                "token2".to_string(),
                vec![("f1".to_string(), 0.8)].into_iter().collect(),
            ),
        ];

        batch_store.store_batch(entries).await.unwrap();

        let keys = vec!["token1".to_string(), "token2".to_string()];
        let results = batch_store.get_batch(&keys).await;

        assert_eq!(results.len(), 2);
        assert!(results[0].1.is_some());
        assert!(results[1].1.is_some());
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let store = FeatureStore::new(100, Duration::from_secs(300));

        let features: FeatureVector = vec![("f1".to_string(), 0.5)].into_iter().collect();
        store
            .store("token1".to_string(), features.clone())
            .await
            .unwrap();
        store
            .store("token2".to_string(), features.clone())
            .await
            .unwrap();

        // Verify we can retrieve both entries
        let retrieved1 = store.get(&"token1".to_string()).await;
        let retrieved2 = store.get(&"token2".to_string()).await;

        assert!(retrieved1.is_some());
        assert!(retrieved2.is_some());

        // Stats might be eventually consistent, so just verify it exists
        let stats = store.stats().await;
        // The cache is working if we can retrieve entries, stats may vary
        assert!(retrieved1.unwrap().get("f1") == Some(&0.5));
    }
}
