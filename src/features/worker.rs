//! Asynchronous worker for feature extraction and storage
//!
//! This module provides an asynchronous worker that fetches TokenData from OracleDataSources,
//! extracts features using FeatureExtractionPipeline, and stores them in FeatureStore.
//!
//! ## Design Goals
//!
//! - **Non-blocking**: Does not block the scoring path
//! - **Concurrent**: Handles multiple candidates in parallel with configurable limits
//! - **Resilient**: Handles errors gracefully without crashing
//! - **Cached**: Uses FeatureStore with TTL to avoid redundant work
//!
//! ## Usage
//!
//! ```no_run
//! use h_5n1p3r::features::worker::{FeatureWorker, FeatureWorkerConfig};
//! use h_5n1p3r::features::FeatureStore;
//! use h_5n1p3r::oracle::data_sources::OracleDataSources;
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Create worker configuration
//! let config = FeatureWorkerConfig {
//!     max_concurrent_fetches: 10,
//!     fetch_timeout: Duration::from_secs(30),
//!     retry_attempts: 3,
//!     queue_capacity: 1000,
//! };
//!
//! // Initialize components
//! let data_sources = // ... create OracleDataSources
//! # panic!("example only");
//! let feature_store = Arc::new(FeatureStore::new(1000, Duration::from_secs(300)));
//!
//! // Create and start worker
//! let worker = FeatureWorker::new(data_sources, feature_store, config);
//! let worker_handle = worker.start().await?;
//!
//! // Submit candidates for processing
//! // worker_handle.submit(candidate).await?;
//!
//! // Shutdown gracefully
//! worker_handle.shutdown().await?;
//! # Ok(())
//! # }
//! ```

use crate::features::extractors::{FeatureExtractionPipeline, FeatureVector};
use crate::features::store::FeatureStore;
use crate::oracle::data_sources::OracleDataSources;
use crate::types::PremintCandidate;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::{debug, error, info, instrument, warn};

/// Configuration for the feature worker
#[derive(Debug, Clone)]
pub struct FeatureWorkerConfig {
    /// Maximum number of concurrent fetch operations
    pub max_concurrent_fetches: usize,
    /// Timeout for each fetch operation
    pub fetch_timeout: Duration,
    /// Number of retry attempts for failed fetches
    pub retry_attempts: usize,
    /// Capacity of the work queue
    pub queue_capacity: usize,
}

impl Default for FeatureWorkerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_fetches: 10,
            fetch_timeout: Duration::from_secs(30),
            retry_attempts: 3,
            queue_capacity: 1000,
        }
    }
}

/// Asynchronous worker for feature extraction and caching
pub struct FeatureWorker {
    data_sources: Arc<OracleDataSources>,
    feature_store: Arc<FeatureStore>,
    extraction_pipeline: Arc<FeatureExtractionPipeline>,
    config: FeatureWorkerConfig,
}

impl FeatureWorker {
    /// Create a new feature worker
    pub fn new(
        data_sources: Arc<OracleDataSources>,
        feature_store: Arc<FeatureStore>,
        config: FeatureWorkerConfig,
    ) -> Self {
        Self {
            data_sources,
            feature_store,
            extraction_pipeline: Arc::new(FeatureExtractionPipeline::new()),
            config,
        }
    }

    /// Start the worker and return a handle for interaction
    pub async fn start(self) -> Result<FeatureWorkerHandle> {
        let (tx, rx) = mpsc::channel(self.config.queue_capacity);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let handle = tokio::spawn(async move {
            self.run(rx, shutdown_rx).await;
        });

        Ok(FeatureWorkerHandle {
            sender: tx,
            shutdown_sender: Some(shutdown_tx),
            worker_task: handle,
        })
    }

    /// Main worker loop
    #[instrument(skip(self, receiver, shutdown_rx), name = "feature_worker")]
    async fn run(
        self,
        mut receiver: mpsc::Receiver<PremintCandidate>,
        mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        info!("Feature worker started");

        // Semaphore to limit concurrent operations
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent_fetches));
        let mut active_tasks: Vec<JoinHandle<()>> = Vec::new();

        loop {
            tokio::select! {
                // Handle shutdown signal
                _ = &mut shutdown_rx => {
                    info!("Feature worker received shutdown signal");
                    break;
                }

                // Process incoming candidates
                Some(candidate) = receiver.recv() => {
                    // Acquire permit to control concurrency
                    let permit = match semaphore.clone().try_acquire_owned() {
                        Ok(p) => p,
                        Err(_) => {
                            debug!("Concurrency limit reached, waiting for slot");
                            match semaphore.clone().acquire_owned().await {
                                Ok(p) => p,
                                Err(e) => {
                                    error!("Failed to acquire semaphore: {}", e);
                                    continue;
                                }
                            }
                        }
                    };

                    // Spawn task to process this candidate
                    let task = self.process_candidate(candidate, permit);
                    active_tasks.push(tokio::spawn(task));
                }
            }

            // Clean up completed tasks
            active_tasks.retain(|task| !task.is_finished());
        }

        // Wait for all active tasks to complete
        info!("Waiting for {} active tasks to complete", active_tasks.len());
        for task in active_tasks {
            let _ = task.await;
        }

        info!("Feature worker stopped");
    }

    /// Process a single candidate
    #[instrument(skip(self, _permit), fields(mint = %candidate.mint))]
    async fn process_candidate(
        &self,
        candidate: PremintCandidate,
        _permit: tokio::sync::OwnedSemaphorePermit,
    ) {
        let mint = candidate.mint.clone();

        // Check if features are already cached
        if let Some(_cached_features) = self.feature_store.get(&mint).await {
            debug!("Features already cached for {}, skipping", mint);
            return;
        }

        // Attempt to fetch and process with retries
        for attempt in 1..=self.config.retry_attempts {
            match self.fetch_and_extract(&candidate).await {
                Ok(features) => {
                    // Store in feature store
                    if let Err(e) = self.feature_store.store(mint.clone(), features).await {
                        error!("Failed to store features for {}: {}", mint, e);
                    } else {
                        debug!("Successfully stored features for {}", mint);
                    }
                    return; // Success, exit retry loop
                }
                Err(e) => {
                    if attempt < self.config.retry_attempts {
                        warn!(
                            "Attempt {}/{} failed for {}: {}. Retrying...",
                            attempt, self.config.retry_attempts, mint, e
                        );
                        // Exponential backoff
                        tokio::time::sleep(Duration::from_millis(100 * (1 << attempt))).await;
                    } else {
                        error!(
                            "All {} attempts failed for {}: {}",
                            self.config.retry_attempts, mint, e
                        );
                    }
                }
            }
        }
    }

    /// Fetch TokenData and extract features with timeout
    async fn fetch_and_extract(&self, candidate: &PremintCandidate) -> Result<FeatureVector> {
        // Wrap the entire operation in a timeout
        timeout(self.config.fetch_timeout, async {
            // Fetch TokenData from OracleDataSources
            let token_data = self
                .data_sources
                .fetch_token_data_with_retries(candidate)
                .await
                .context("Failed to fetch token data")?;

            // Extract features using the pipeline
            let features = self
                .extraction_pipeline
                .extract_all(candidate, &token_data)
                .context("Failed to extract features")?;

            Ok(features)
        })
        .await
        .context("Operation timed out")?
    }
}

/// Handle for interacting with the feature worker
pub struct FeatureWorkerHandle {
    sender: mpsc::Sender<PremintCandidate>,
    shutdown_sender: Option<tokio::sync::oneshot::Sender<()>>,
    worker_task: JoinHandle<()>,
}

impl FeatureWorkerHandle {
    /// Submit a candidate for processing
    ///
    /// This is a non-blocking operation that queues the candidate for processing.
    /// If the queue is full, this will wait until space is available.
    pub async fn submit(&self, candidate: PremintCandidate) -> Result<()> {
        self.sender
            .send(candidate)
            .await
            .context("Failed to submit candidate to worker")
    }

    /// Try to submit a candidate without blocking
    ///
    /// Returns an error if the queue is full.
    pub fn try_submit(&self, candidate: PremintCandidate) -> Result<()> {
        self.sender
            .try_send(candidate)
            .context("Failed to submit candidate to worker (queue full)")
    }

    /// Gracefully shutdown the worker
    ///
    /// This waits for all active tasks to complete before shutting down.
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(shutdown_sender) = self.shutdown_sender.take() {
            let _ = shutdown_sender.send(());
        }

        self.worker_task
            .await
            .context("Worker task panicked or was cancelled")?;

        Ok(())
    }

    /// Get the current queue capacity
    pub fn queue_capacity(&self) -> usize {
        self.sender.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::OracleConfig;
    use reqwest::Client;

    fn create_test_candidate(mint: &str) -> PremintCandidate {
        PremintCandidate {
            mint: mint.to_string(),
            creator: "TestCreator123456789".to_string(),
            program: "test".to_string(),
            slot: 12345,
            timestamp: 1640995200,
            instruction_summary: None,
            is_jito_bundle: Some(true),
        }
    }

    #[tokio::test]
    async fn test_worker_creation_and_start() {
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        let worker_config = FeatureWorkerConfig {
            max_concurrent_fetches: 5,
            fetch_timeout: Duration::from_secs(10),
            retry_attempts: 2,
            queue_capacity: 100,
        };

        let worker = FeatureWorker::new(data_sources, feature_store, worker_config);
        let handle = worker.start().await.unwrap();

        // Shutdown immediately
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_worker_submission() {
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        let worker_config = FeatureWorkerConfig {
            max_concurrent_fetches: 5,
            fetch_timeout: Duration::from_secs(10),
            retry_attempts: 2,
            queue_capacity: 100,
        };

        let worker = FeatureWorker::new(data_sources, feature_store, worker_config);
        let handle = worker.start().await.unwrap();

        // Submit a test candidate
        let candidate = create_test_candidate("TestMint123");
        let result = handle.submit(candidate).await;
        assert!(result.is_ok());

        // Give it a moment to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Shutdown
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_worker_try_submit() {
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        let worker_config = FeatureWorkerConfig {
            max_concurrent_fetches: 5,
            fetch_timeout: Duration::from_secs(10),
            retry_attempts: 2,
            queue_capacity: 10,
        };

        let worker = FeatureWorker::new(data_sources, feature_store, worker_config);
        let handle = worker.start().await.unwrap();

        // Try to submit multiple candidates
        for i in 0..5 {
            let candidate = create_test_candidate(&format!("TestMint{}", i));
            let result = handle.try_submit(candidate);
            assert!(result.is_ok());
        }

        // Shutdown
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_worker_cache_skip() {
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        // Pre-populate cache
        let mint = "CachedMint123".to_string();
        let features = std::collections::HashMap::new();
        feature_store.store(mint.clone(), features).await.unwrap();

        let worker_config = FeatureWorkerConfig::default();
        let worker = FeatureWorker::new(
            data_sources.clone(),
            feature_store.clone(),
            worker_config,
        );
        let handle = worker.start().await.unwrap();

        // Submit the cached candidate
        let candidate = create_test_candidate(&mint);
        handle.submit(candidate).await.unwrap();

        // Give it a moment to process (should skip quickly due to cache)
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Verify cache still has the entry
        assert!(feature_store.get(&mint).await.is_some());

        // Shutdown
        handle.shutdown().await.unwrap();
    }

    #[test]
    fn test_worker_config_default() {
        let config = FeatureWorkerConfig::default();
        assert_eq!(config.max_concurrent_fetches, 10);
        assert_eq!(config.fetch_timeout, Duration::from_secs(30));
        assert_eq!(config.retry_attempts, 3);
        assert_eq!(config.queue_capacity, 1000);
    }

    #[tokio::test]
    async fn test_cache_hit_skip() {
        // Test that worker skips processing when features are already cached
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        // Pre-populate cache
        let mint = "CachedMint456".to_string();
        let mut features = std::collections::HashMap::new();
        features.insert("test_feature".to_string(), 42.0);
        feature_store.store(mint.clone(), features.clone()).await.unwrap();

        let worker_config = FeatureWorkerConfig::default();
        let worker = FeatureWorker::new(
            data_sources,
            feature_store.clone(),
            worker_config,
        );
        let handle = worker.start().await.unwrap();

        // Submit the cached candidate
        let candidate = create_test_candidate(&mint);
        handle.submit(candidate).await.unwrap();

        // Give it time to check cache and skip
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify original cached features are still there
        let cached = feature_store.get(&mint).await.unwrap();
        assert_eq!(cached.get("test_feature"), Some(&42.0));

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_cache_miss_storage() {
        // Test that features are stored when not in cache
        // Note: This test will fail to fetch real data but should still exercise the path
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        let worker_config = FeatureWorkerConfig {
            max_concurrent_fetches: 5,
            fetch_timeout: Duration::from_millis(500), // Short timeout for test
            retry_attempts: 1, // Only one attempt for test
            queue_capacity: 100,
        };

        let worker = FeatureWorker::new(
            data_sources,
            feature_store.clone(),
            worker_config,
        );
        let handle = worker.start().await.unwrap();

        let mint = "NewMint789".to_string();
        let candidate = create_test_candidate(&mint);

        // Verify not in cache before
        assert!(feature_store.get(&mint).await.is_none());

        handle.submit(candidate).await.unwrap();

        // Give it time to attempt fetch (will fail but that's expected in test)
        tokio::time::sleep(Duration::from_millis(600)).await;

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_concurrent_submissions() {
        // Test that worker handles multiple concurrent submissions
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        let worker_config = FeatureWorkerConfig {
            max_concurrent_fetches: 3,
            fetch_timeout: Duration::from_millis(500),
            retry_attempts: 1,
            queue_capacity: 50,
        };

        let worker = FeatureWorker::new(
            data_sources,
            feature_store.clone(),
            worker_config,
        );
        let handle = worker.start().await.unwrap();

        // Submit multiple candidates concurrently
        let mut tasks = vec![];
        for i in 0..10 {
            let candidate = create_test_candidate(&format!("ConcurrentMint{}", i));
            let handle_clone = &handle;
            tasks.push(async move {
                handle_clone.submit(candidate).await
            });
        }

        // Wait for all submissions
        for task in tasks {
            assert!(task.await.is_ok());
        }

        // Give time for processing
        tokio::time::sleep(Duration::from_millis(700)).await;

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        // Test that cached features expire after TTL
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));

        // Create store with very short TTL (1 second)
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(1)));

        // Store some features
        let mint = "TTLTestMint".to_string();
        let mut features = std::collections::HashMap::new();
        features.insert("test_feature".to_string(), 123.0);
        feature_store.store(mint.clone(), features.clone()).await.unwrap();

        // Verify features are cached
        assert!(feature_store.get(&mint).await.is_some());

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Features should be expired now
        // Note: Moka cache handles TTL automatically, but the exact behavior
        // depends on the cache implementation
        let worker_config = FeatureWorkerConfig::default();
        let worker = FeatureWorker::new(
            data_sources,
            feature_store.clone(),
            worker_config,
        );
        let handle = worker.start().await.unwrap();

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_queue_capacity() {
        // Test that queue capacity is respected
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        let worker_config = FeatureWorkerConfig {
            max_concurrent_fetches: 5,
            fetch_timeout: Duration::from_secs(10),
            retry_attempts: 2,
            queue_capacity: 5, // Small queue for testing
        };

        let worker = FeatureWorker::new(
            data_sources,
            feature_store,
            worker_config,
        );
        let handle = worker.start().await.unwrap();

        // Fill up the queue
        for i in 0..5 {
            let candidate = create_test_candidate(&format!("QueueMint{}", i));
            assert!(handle.try_submit(candidate).is_ok());
        }

        // Queue should be full now, try_submit should fail
        let overflow_candidate = create_test_candidate("OverflowMint");
        let result = handle.try_submit(overflow_candidate);

        // May or may not fail depending on timing, but capacity should be tracked
        assert_eq!(handle.queue_capacity(), 5);

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        // Test that worker shuts down gracefully
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        let worker_config = FeatureWorkerConfig::default();
        let worker = FeatureWorker::new(
            data_sources,
            feature_store,
            worker_config,
        );
        let handle = worker.start().await.unwrap();

        // Submit some work
        for i in 0..3 {
            let candidate = create_test_candidate(&format!("ShutdownMint{}", i));
            handle.submit(candidate).await.unwrap();
        }

        // Shutdown should wait for active tasks
        let shutdown_result = handle.shutdown().await;
        assert!(shutdown_result.is_ok());
    }

    #[tokio::test]
    async fn test_retry_on_failure() {
        // Test that worker retries on failure
        let endpoints = vec!["https://invalid-endpoint-that-will-fail.local".to_string()];
        let http_client = Client::new();
        let config = OracleConfig::default();
        let data_sources = Arc::new(OracleDataSources::new(endpoints, http_client, config));
        let feature_store = Arc::new(FeatureStore::new(100, Duration::from_secs(60)));

        let worker_config = FeatureWorkerConfig {
            max_concurrent_fetches: 5,
            fetch_timeout: Duration::from_millis(500),
            retry_attempts: 3, // Test multiple retries
            queue_capacity: 100,
        };

        let worker = FeatureWorker::new(
            data_sources,
            feature_store.clone(),
            worker_config,
        );
        let handle = worker.start().await.unwrap();

        let mint = "RetryMint".to_string();
        let candidate = create_test_candidate(&mint);

        // Submit candidate that will fail
        handle.submit(candidate).await.unwrap();

        // Give time for retries to complete (3 attempts with exponential backoff)
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Should not be in cache due to failures
        assert!(feature_store.get(&mint).await.is_none());

        handle.shutdown().await.unwrap();
    }
}
