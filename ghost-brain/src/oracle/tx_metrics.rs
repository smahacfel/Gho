//! Transaction Metrics Module
//!
//! Provides real-time transaction metrics for Oracle scoring.
//! This module replaces the hardcoded MOCK values with actual transaction data
//! collected during the analysis window.
//!
//! ## Purpose
//! The Oracle Brain requires accurate transaction metrics for:
//! - ULVF (Liquidity Vector Field) analysis
//! - POVC (Pump/Organic/Volume Cluster) classification
//! - QASS wave generation
//! - Market coherence calculations
//!
//! Without real data, the Oracle produces static scores (e.g., QASS=88%, λ=0.728)
//! because MOCK values always produce the same cluster classification.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Real-time transaction metrics collected during the analysis window
///
/// This structure replaces hardcoded MOCK values in hyper_prediction.rs:
/// - tx_count: 10, 20, 15 → actual transaction counts
/// - unique_addrs: 5, 10, 8 → actual unique signer counts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionMetrics {
    /// Total number of transactions observed in the analysis window
    pub tx_count: usize,

    /// Number of unique wallet addresses (signers) observed
    pub unique_addrs: usize,

    /// Total SOL volume in the analysis window
    pub total_volume_sol: f64,

    /// Number of buy transactions
    pub buy_count: usize,

    /// Number of sell transactions
    pub sell_count: usize,

    /// SOL volume from buys
    pub buy_volume_sol: f64,

    /// SOL volume from sells
    pub sell_volume_sol: f64,

    /// Maximum single transaction size in SOL
    pub max_tx_sol: f64,

    /// Raw per-transaction volumes in SOL (preserves microstructure)
    pub volumes_sol: Vec<f64>,

    /// Raw per-transaction direction flags (true = buy, false = sell)
    pub is_buys: Vec<bool>,

    /// Average time between transactions in milliseconds
    pub avg_interval_ms: f64,

    /// Standard deviation of intervals (for bot detection)
    pub interval_std_dev: f64,

    /// Source of the interval measurement (real timestamps, slot estimation, or unknown)
    #[serde(default)]
    pub interval_source: IntervalSource,

    /// Whether developer wallet activity was detected
    pub has_dev_activity: bool,

    /// Developer wallet's SOL volume if detected
    pub dev_volume_sol: f64,
}

/// Provenance for interval calculations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntervalSource {
    TimestampDelta,
    SlotEstimated,
    Unknown,
}

impl Default for IntervalSource {
    fn default() -> Self {
        IntervalSource::Unknown
    }
}

impl TransactionMetrics {
    /// Create new empty metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder method: set tx_count
    pub fn with_tx_count(mut self, count: usize) -> Self {
        self.tx_count = count;
        self
    }

    /// Builder method: set unique_addrs
    pub fn with_unique_addrs(mut self, count: usize) -> Self {
        self.unique_addrs = count;
        self
    }

    /// Builder method: set volumes from a list (calculates total, max, etc.)
    pub fn with_volumes(mut self, volumes: Vec<f64>) -> Self {
        self.total_volume_sol = volumes.iter().sum();
        self.max_tx_sol = volumes.iter().cloned().fold(0.0_f64, f64::max);
        self.volumes_sol = volumes;
        // Default to alternating buy/sell if no directions provided
        self.is_buys = self
            .volumes_sol
            .iter()
            .enumerate()
            .map(|(i, _)| i % 2 == 0)
            .collect();
        self.tx_count = self.volumes_sol.len();
        self.buy_count = (self.tx_count + 1) / 2;
        self.sell_count = self.tx_count / 2;
        // Assume half buys half sells for simple builder
        self.buy_volume_sol = self
            .is_buys
            .iter()
            .zip(self.volumes_sol.iter())
            .filter_map(|(b, v)| if *b { Some(*v) } else { None })
            .sum();
        self.sell_volume_sol = self
            .is_buys
            .iter()
            .zip(self.volumes_sol.iter())
            .filter_map(|(b, v)| if !*b { Some(*v) } else { None })
            .sum();
        self
    }

    /// Builder method: set interval coefficient of variation
    pub fn with_interval_cv(mut self, cv: f64) -> Self {
        // Store CV by setting std_dev = cv * avg_interval_ms
        // Assume avg_interval_ms = 100ms as baseline
        self.avg_interval_ms = 100.0;
        self.interval_std_dev = cv * 100.0;
        self.interval_source = IntervalSource::Unknown;
        self
    }

    /// Calculate the unique address ratio
    pub fn unique_ratio(&self) -> f64 {
        if self.tx_count == 0 {
            0.0
        } else {
            self.unique_addrs as f64 / self.tx_count as f64
        }
    }

    /// Create metrics from collected transaction timestamps and signers
    pub fn from_transactions(
        timestamps_ms: &[u64],
        signers: &[String],
        volumes_sol: &[f64],
        is_buys: &[bool],
    ) -> Self {
        let tx_count = timestamps_ms.len();

        // Calculate unique addresses
        let unique_set: HashSet<&String> = signers.iter().collect();
        let unique_addrs = unique_set.len();

        // Calculate volumes
        let total_volume_sol: f64 = volumes_sol.iter().sum();
        let max_tx_sol = volumes_sol.iter().cloned().fold(0.0_f64, f64::max);

        // Calculate buy/sell breakdown
        let mut buy_count = 0;
        let mut sell_count = 0;
        let mut buy_volume_sol = 0.0;
        let mut sell_volume_sol = 0.0;

        for (i, &is_buy) in is_buys.iter().enumerate() {
            let vol = volumes_sol.get(i).copied().unwrap_or(0.0);
            if is_buy {
                buy_count += 1;
                buy_volume_sol += vol;
            } else {
                sell_count += 1;
                sell_volume_sol += vol;
            }
        }

        // Calculate timing metrics
        let (avg_interval_ms, interval_std_dev) = Self::calculate_interval_stats(timestamps_ms);

        Self {
            tx_count,
            unique_addrs,
            volumes_sol: volumes_sol.to_vec(),
            is_buys: is_buys.to_vec(),
            total_volume_sol,
            buy_count,
            sell_count,
            buy_volume_sol,
            sell_volume_sol,
            max_tx_sol,
            avg_interval_ms,
            interval_std_dev,
            has_dev_activity: false,
            dev_volume_sol: 0.0,
            interval_source: IntervalSource::TimestampDelta,
        }
    }

    /// Iterate over per-transaction (volume, direction) pairs
    pub fn iter_transactions(&self) -> impl Iterator<Item = (f64, bool)> + '_ {
        self.volumes_sol
            .iter()
            .copied()
            .zip(self.is_buys.iter().copied())
    }

    /// Calculate interval statistics from timestamps
    fn calculate_interval_stats(timestamps_ms: &[u64]) -> (f64, f64) {
        if timestamps_ms.len() < 2 {
            return (0.0, 0.0);
        }

        let mut sorted: Vec<u64> = timestamps_ms.to_vec();
        sorted.sort();

        let intervals: Vec<f64> = sorted.windows(2).map(|w| (w[1] - w[0]) as f64).collect();

        if intervals.is_empty() {
            return (0.0, 0.0);
        }

        let avg = intervals.iter().sum::<f64>() / intervals.len() as f64;

        let variance =
            intervals.iter().map(|&x| (x - avg).powi(2)).sum::<f64>() / intervals.len() as f64;

        let std_dev = variance.sqrt();

        (avg, std_dev)
    }

    /// Calculate the buy pressure ratio (0.0 = all sells, 1.0 = all buys)
    pub fn buy_pressure_ratio(&self) -> f64 {
        let total = self.buy_count + self.sell_count;
        if total == 0 {
            0.5 // Neutral when no transactions
        } else {
            self.buy_count as f64 / total as f64
        }
    }

    /// Calculate volume buy pressure (based on SOL amounts)
    pub fn volume_buy_pressure(&self) -> f64 {
        let total = self.buy_volume_sol + self.sell_volume_sol;
        if total < 0.001 {
            0.5 // Neutral when no volume
        } else {
            self.buy_volume_sol / total
        }
    }

    /// Coefficient of variation for interval timing (bot detection)
    /// Low CV (<0.3) suggests bot activity (regular intervals)
    /// High CV (>0.5) suggests organic human activity (irregular intervals)
    pub fn interval_cv(&self) -> f64 {
        if self.avg_interval_ms < 1.0 {
            0.5 // Neutral when no data
        } else {
            (self.interval_std_dev / self.avg_interval_ms).min(2.0)
        }
    }

    /// Check if metrics indicate potential bot activity
    pub fn is_bot_like(&self) -> bool {
        // Bot indicators:
        // 1. Very regular intervals (low CV)
        // 2. Many transactions from few wallets
        // 3. Very fast transactions

        let cv = self.interval_cv();
        let wallet_ratio = if self.tx_count > 0 {
            self.unique_addrs as f64 / self.tx_count as f64
        } else {
            1.0
        };

        cv < 0.25 || wallet_ratio < 0.3
    }

    /// Check if metrics suggest organic activity
    pub fn is_organic(&self) -> bool {
        // Organic indicators:
        // 1. Irregular intervals (high CV)
        // 2. Many unique wallets
        // 3. Mixed buy/sell activity

        let cv = self.interval_cv();
        let wallet_ratio = if self.tx_count > 0 {
            self.unique_addrs as f64 / self.tx_count as f64
        } else {
            0.0
        };
        let buy_ratio = self.buy_pressure_ratio();

        cv > 0.4 && wallet_ratio > 0.5 && buy_ratio > 0.3 && buy_ratio < 0.9
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_metrics() {
        let metrics = TransactionMetrics::new();
        assert_eq!(metrics.tx_count, 0);
        assert_eq!(metrics.unique_addrs, 0);
        assert_eq!(metrics.buy_pressure_ratio(), 0.5);
    }

    #[test]
    fn test_from_transactions() {
        let timestamps = vec![1000, 1100, 1250, 1400, 1600];
        let signers = vec![
            "wallet1".to_string(),
            "wallet2".to_string(),
            "wallet1".to_string(),
            "wallet3".to_string(),
            "wallet2".to_string(),
        ];
        let volumes = vec![1.0, 2.0, 0.5, 1.5, 3.0];
        let is_buys = vec![true, true, false, true, false];

        let metrics =
            TransactionMetrics::from_transactions(&timestamps, &signers, &volumes, &is_buys);

        assert_eq!(metrics.tx_count, 5);
        assert_eq!(metrics.unique_addrs, 3);
        assert_eq!(metrics.buy_count, 3);
        assert_eq!(metrics.sell_count, 2);
        assert!((metrics.total_volume_sol - 8.0).abs() < 0.01);
    }

    #[test]
    fn test_interval_cv_bot_detection() {
        // Regular intervals (bot-like)
        let bot_timestamps: Vec<u64> = (0..10).map(|i| i * 100).collect();
        let (avg, std) = TransactionMetrics::calculate_interval_stats(&bot_timestamps);
        let cv = std / avg;
        assert!(cv < 0.1, "Bot-like intervals should have low CV: {}", cv);

        // Irregular intervals (human-like)
        let human_timestamps = vec![0, 150, 200, 450, 500, 800, 850, 1200, 1500, 1550];
        let (avg2, std2) = TransactionMetrics::calculate_interval_stats(&human_timestamps);
        let cv2 = std2 / avg2;
        assert!(
            cv2 > 0.3,
            "Human-like intervals should have higher CV: {}",
            cv2
        );
    }
}
