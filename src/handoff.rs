//! Bounded mpsc channel with backpressure and priority logic

use super::config::DropPolicy;
use super::extractor::PremintCandidate;
use super::telemetry::{HandoffDiagnostics, SnifferMetrics};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{debug, warn};

/// Handoff result
pub enum HandoffResult {
    /// Successfully sent
    Sent,
    /// Dropped due to full buffer
    Dropped,
    /// Backpressure applied (would block)
    Backpressure,
}

/// Try to send a candidate through the channel with backpressure handling
/// This is a HOT-PATH function - uses try_send to avoid blocking
/// Note: Records send latency, not actual queue wait time (since try_send is non-blocking)
#[inline]
pub fn try_send_candidate(
    tx: &mpsc::Sender<PremintCandidate>,
    candidate: PremintCandidate,
    metrics: &Arc<SnifferMetrics>,
    diagnostics: Option<&Arc<HandoffDiagnostics>>,
) -> HandoffResult {
    match tx.try_send(candidate.clone()) {
        Ok(_) => {
            // Successfully sent
            metrics.candidates_sent.fetch_add(1, Ordering::Relaxed);

            if candidate.is_high_priority() {
                metrics.high_priority_sent.fetch_add(1, Ordering::Relaxed);
            } else {
                metrics.low_priority_sent.fetch_add(1, Ordering::Relaxed);
            }

            HandoffResult::Sent
        }
        Err(mpsc::error::TrySendError::Full(dropped_candidate)) => {
            // Channel is full - apply backpressure
            metrics.dropped_full_buffer.fetch_add(1, Ordering::Relaxed);
            metrics.backpressure_events.fetch_add(1, Ordering::Relaxed);

            let is_high_priority = dropped_candidate.is_high_priority();
            if is_high_priority {
                metrics
                    .high_priority_dropped
                    .fetch_add(1, Ordering::Relaxed);
                warn!("Dropped HIGH priority candidate due to full buffer");
            }

            // Track drop in diagnostics
            if let Some(diag) = diagnostics {
                diag.record_drop(is_high_priority);
            }

            HandoffResult::Dropped
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            // Channel closed - this is a critical error
            warn!("Channel closed - cannot send candidate");
            HandoffResult::Dropped
        }
    }
}

/// Batch sender for efficient bulk transmission
pub struct BatchSender {
    tx: mpsc::Sender<PremintCandidate>,
    batch: Vec<PremintCandidate>,
    batch_size: usize,
    batch_timeout: Duration,
    last_send: Instant,
    metrics: Arc<SnifferMetrics>,
    diagnostics: Option<Arc<HandoffDiagnostics>>,
}

impl BatchSender {
    /// Create a new batch sender
    pub fn new(
        tx: mpsc::Sender<PremintCandidate>,
        batch_size: usize,
        batch_timeout: Duration,
        metrics: Arc<SnifferMetrics>,
    ) -> Self {
        Self {
            tx,
            batch: Vec::with_capacity(batch_size),
            batch_size,
            batch_timeout,
            last_send: Instant::now(),
            metrics,
            diagnostics: None,
        }
    }

    /// Create a new batch sender with diagnostics
    pub fn with_diagnostics(
        tx: mpsc::Sender<PremintCandidate>,
        batch_size: usize,
        batch_timeout: Duration,
        metrics: Arc<SnifferMetrics>,
        diagnostics: Arc<HandoffDiagnostics>,
    ) -> Self {
        Self {
            tx,
            batch: Vec::with_capacity(batch_size),
            batch_size,
            batch_timeout,
            last_send: Instant::now(),
            metrics,
            diagnostics: Some(diagnostics),
        }
    }

    /// Add a candidate to the batch
    /// Returns true if batch was flushed
    #[inline]
    pub fn add(&mut self, candidate: PremintCandidate) -> bool {
        self.batch.push(candidate);

        // Check if we should flush
        let should_flush =
            self.batch.len() >= self.batch_size || self.last_send.elapsed() >= self.batch_timeout;

        if should_flush {
            self.flush_sync();
            true
        } else {
            false
        }
    }

    /// Flush the batch synchronously (HOT-PATH)
    /// Uses try_send to avoid blocking
    #[inline]
    pub fn flush_sync(&mut self) {
        if self.batch.is_empty() {
            return;
        }

        // Send each candidate in batch and track send time
        for candidate in self.batch.drain(..) {
            let send_start = Instant::now();
            try_send_candidate(
                &self.tx,
                candidate,
                &self.metrics,
                self.diagnostics.as_ref(),
            );
            // Record batch send latency (not actual queue wait since try_send is non-blocking)
            if let Some(diag) = &self.diagnostics {
                let send_latency_us = send_start.elapsed().as_micros() as u64;
                diag.record_queue_wait(send_latency_us);
            }
        }

        self.last_send = Instant::now();
    }

    /// Check if batch should be flushed due to timeout
    #[inline]
    pub fn should_flush_timeout(&self) -> bool {
        !self.batch.is_empty() && self.last_send.elapsed() >= self.batch_timeout
    }

    /// Get current batch size
    #[inline]
    pub fn len(&self) -> usize {
        self.batch.len()
    }

    /// Check if batch is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }
}

/// Priority queue handler for high/low priority candidates
pub struct PriorityHandler {
    high_priority_tx: mpsc::Sender<PremintCandidate>,
    low_priority_tx: mpsc::Sender<PremintCandidate>,
    metrics: Arc<SnifferMetrics>,
}

impl PriorityHandler {
    /// Create a new priority handler
    pub fn new(
        high_priority_tx: mpsc::Sender<PremintCandidate>,
        low_priority_tx: mpsc::Sender<PremintCandidate>,
        metrics: Arc<SnifferMetrics>,
    ) -> Self {
        Self {
            high_priority_tx,
            low_priority_tx,
            metrics,
        }
    }

    /// Send candidate to appropriate priority queue
    #[inline]
    pub fn send(&self, candidate: PremintCandidate) -> HandoffResult {
        if candidate.is_high_priority() {
            try_send_candidate(&self.high_priority_tx, candidate, &self.metrics, None)
        } else {
            try_send_candidate(&self.low_priority_tx, candidate, &self.metrics, None)
        }
    }
}

/// Backpressure policy configuration
pub struct BackpressurePolicy {
    /// Drop policy when channel is full
    pub drop_policy: DropPolicy,
    /// Maximum retry attempts for high priority
    pub max_retries_high: u8,
    /// Retry delay in microseconds
    pub retry_delay_us: u64,
    /// Diagnostics for adaptive policy
    pub diagnostics: Option<Arc<HandoffDiagnostics>>,
    /// High congestion threshold (microseconds)
    pub high_threshold_us: f64,
    /// Low congestion threshold (microseconds)
    pub low_threshold_us: f64,
}

impl BackpressurePolicy {
    /// Create a new backpressure policy
    pub fn new(drop_policy: DropPolicy, max_retries_high: u8, retry_delay_us: u64) -> Self {
        Self {
            drop_policy,
            max_retries_high,
            retry_delay_us,
            diagnostics: None,
            high_threshold_us: 1000.0,
            low_threshold_us: 100.0,
        }
    }

    /// Create a new backpressure policy with diagnostics for adaptive behavior
    pub fn with_diagnostics(
        drop_policy: DropPolicy,
        max_retries_high: u8,
        retry_delay_us: u64,
        diagnostics: Arc<HandoffDiagnostics>,
        high_threshold_us: f64,
        low_threshold_us: f64,
    ) -> Self {
        Self {
            drop_policy,
            max_retries_high,
            retry_delay_us,
            diagnostics: Some(diagnostics),
            high_threshold_us,
            low_threshold_us,
        }
    }

    /// Determine adaptive drop policy based on diagnostics
    /// Returns the policy to use, potentially adjusted based on current conditions
    pub fn adaptive_policy(&self) -> DropPolicy {
        if let Some(diag) = &self.diagnostics {
            // Get average queue wait time
            if let Some(avg_wait) = diag.avg_queue_wait() {
                // If queue wait is very high (>high_threshold), switch to more aggressive dropping
                if avg_wait > self.high_threshold_us {
                    debug!(
                        "High queue wait detected ({:.2}μs), using DropNewest",
                        avg_wait
                    );
                    return DropPolicy::DropNewest;
                }

                // If queue wait is low (<low_threshold), we can afford to block occasionally
                if avg_wait < self.low_threshold_us {
                    debug!("Low queue wait ({:.2}μs), using Block policy", avg_wait);
                    return DropPolicy::Block;
                }
            }
        }

        // Default to configured policy
        self.drop_policy
    }

    /// Apply backpressure policy for a candidate
    /// This is NOT a hot-path function - only called when channel is full
    pub async fn apply(
        &self,
        tx: &mpsc::Sender<PremintCandidate>,
        candidate: PremintCandidate,
        metrics: &Arc<SnifferMetrics>,
    ) -> HandoffResult {
        // Use adaptive policy if diagnostics available
        let policy = self.adaptive_policy();

        match policy {
            DropPolicy::DropNewest => {
                // Drop the current candidate
                debug!("Dropping newest candidate due to backpressure");
                metrics.dropped_full_buffer.fetch_add(1, Ordering::Relaxed);

                if let Some(diag) = &self.diagnostics {
                    diag.record_drop(candidate.is_high_priority());
                }

                HandoffResult::Dropped
            }
            DropPolicy::DropOldest => {
                // This would require a custom queue implementation
                // For now, just drop newest
                debug!("DropOldest not implemented - dropping newest");
                metrics.dropped_full_buffer.fetch_add(1, Ordering::Relaxed);

                if let Some(diag) = &self.diagnostics {
                    diag.record_drop(candidate.is_high_priority());
                }

                HandoffResult::Dropped
            }
            DropPolicy::Block => {
                // Retry with delay for high priority
                if candidate.is_high_priority() {
                    for attempt in 0..self.max_retries_high {
                        tokio::time::sleep(Duration::from_micros(self.retry_delay_us)).await;

                        if let Ok(_) = tx.try_send(candidate.clone()) {
                            metrics.candidates_sent.fetch_add(1, Ordering::Relaxed);
                            metrics.high_priority_sent.fetch_add(1, Ordering::Relaxed);
                            debug!("Sent high priority candidate after {} retries", attempt + 1);
                            return HandoffResult::Sent;
                        }
                    }

                    // Failed after retries
                    metrics.dropped_full_buffer.fetch_add(1, Ordering::Relaxed);
                    metrics
                        .high_priority_dropped
                        .fetch_add(1, Ordering::Relaxed);

                    if let Some(diag) = &self.diagnostics {
                        diag.record_drop(true);
                    }

                    warn!(
                        "Dropped high priority candidate after {} retries",
                        self.max_retries_high
                    );
                    HandoffResult::Dropped
                } else {
                    // Low priority - just drop
                    metrics.dropped_full_buffer.fetch_add(1, Ordering::Relaxed);

                    if let Some(diag) = &self.diagnostics {
                        diag.record_drop(false);
                    }

                    HandoffResult::Dropped
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sniffer::extractor::PriorityLevel;
    use smallvec::SmallVec;
    use solana_sdk::pubkey::Pubkey;

    #[tokio::test]
    async fn test_try_send_candidate() {
        let (tx, mut rx) = mpsc::channel(10);
        let metrics = Arc::new(SnifferMetrics::new());

        let candidate = PremintCandidate::new(
            Pubkey::new_unique(),
            SmallVec::new(),
            1.0,
            1,
            PriorityLevel::High,
        );

        let result = try_send_candidate(&tx, candidate, &metrics, None);
        assert!(matches!(result, HandoffResult::Sent));

        assert!(rx.recv().await.is_some());
        assert_eq!(metrics.candidates_sent.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_batch_sender() {
        let (tx, mut rx) = mpsc::channel(100);
        let metrics = Arc::new(SnifferMetrics::new());
        let mut batch_sender = BatchSender::new(tx, 3, Duration::from_millis(100), metrics);

        for i in 0..5 {
            let candidate = PremintCandidate::new(
                Pubkey::new_unique(),
                SmallVec::new(),
                1.0,
                i,
                PriorityLevel::Low,
            );
            batch_sender.add(candidate);
        }

        // Should have sent 2 batches (3 + 2)
        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 5);
    }
}
