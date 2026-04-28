//! Block-level metrics for trend analysis
//!
//! Tracks per-block statistics to enable trend detection for
//! SecondWaveDetector and Patient Observer strategy.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Snapshot of market state at a specific block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockSnapshot {
    /// Block/slot number
    pub slot: u64,

    /// Transaction count in this block
    pub tx_count: u32,

    /// Unique wallet count in this block
    pub unique_wallets: u32,

    /// Bot transaction count (from MPCF)
    pub bot_tx_count: u32,

    /// Human transaction count (from MPCF)
    pub human_tx_count: u32,

    /// Total volume in SOL
    pub volume_sol: f64,

    /// Average transaction size
    pub avg_tx_size: f64,

    /// Price ratio (relative to initial)
    pub price_ratio: f32,

    /// Timestamp (unix ms)
    pub timestamp_ms: u64,
}

impl BlockSnapshot {
    /// Calculate bot ratio (0.0-1.0)
    pub fn bot_ratio(&self) -> f32 {
        if self.tx_count == 0 {
            0.0
        } else {
            self.bot_tx_count as f32 / self.tx_count as f32
        }
    }

    /// Calculate organic ratio (0.0-1.0)
    pub fn organic_ratio(&self) -> f32 {
        if self.tx_count == 0 {
            0.0
        } else {
            self.human_tx_count as f32 / self.tx_count as f32
        }
    }
}

/// Ring buffer of block snapshots for trend analysis
#[derive(Debug, Clone)]
pub struct BlockMetricsBuffer {
    /// Snapshots (newest at back)
    snapshots: VecDeque<BlockSnapshot>,

    /// Maximum capacity
    capacity: usize,
}

impl BlockMetricsBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            snapshots: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Add a new block snapshot
    pub fn push(&mut self, snapshot: BlockSnapshot) {
        if self.snapshots.len() >= self.capacity {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(snapshot);
    }

    /// Get snapshots from last N blocks
    pub fn last_n(&self, n: usize) -> Vec<&BlockSnapshot> {
        self.snapshots.iter().rev().take(n).collect()
    }

    /// Calculate bot ratio trend (positive = increasing, negative = decreasing)
    pub fn bot_ratio_trend(&self, window: usize) -> f32 {
        let snapshots: Vec<_> = self.last_n(window);
        if snapshots.len() < 2 {
            return 0.0;
        }

        let half_len = snapshots.len() / 2;
        if half_len == 0 {
            return 0.0;
        }

        // Note: last_n returns newest first (reversed), so:
        // snapshots[0..half_len] are the NEWEST blocks
        // snapshots[half_len..] are the OLDER blocks
        // We want: second_half (newest) - first_half (older)
        let first_half: f32 = snapshots[half_len..]
            .iter()
            .map(|s| s.bot_ratio())
            .sum::<f32>()
            / (snapshots.len() - half_len) as f32;
        let second_half: f32 = snapshots[..half_len]
            .iter()
            .map(|s| s.bot_ratio())
            .sum::<f32>()
            / half_len as f32;

        second_half - first_half // Negative = bots decreasing (good)
    }

    /// Calculate unique wallets trend
    pub fn unique_wallets_trend(&self, window: usize) -> f32 {
        let snapshots: Vec<_> = self.last_n(window);
        if snapshots.len() < 2 {
            return 0.0;
        }

        let half_len = snapshots.len() / 2;
        if half_len == 0 {
            return 0.0;
        }

        // Note: last_n returns newest first (reversed), so:
        // snapshots[0..half_len] are the NEWEST blocks
        // snapshots[half_len..] are the OLDER blocks
        let first_half: f32 = snapshots[half_len..]
            .iter()
            .map(|s| s.unique_wallets as f32)
            .sum::<f32>()
            / (snapshots.len() - half_len) as f32;
        let second_half: f32 = snapshots[..half_len]
            .iter()
            .map(|s| s.unique_wallets as f32)
            .sum::<f32>()
            / half_len as f32;

        second_half - first_half // Positive = more unique wallets (good)
    }

    /// Calculate average bot ratio over window
    pub fn avg_bot_ratio(&self, window: usize) -> f32 {
        let snapshots: Vec<_> = self.last_n(window);
        if snapshots.is_empty() {
            return 0.0;
        }
        snapshots.iter().map(|s| s.bot_ratio()).sum::<f32>() / snapshots.len() as f32
    }

    /// Calculate average organic ratio over window
    pub fn avg_organic_ratio(&self, window: usize) -> f32 {
        let snapshots: Vec<_> = self.last_n(window);
        if snapshots.is_empty() {
            return 0.0;
        }
        snapshots.iter().map(|s| s.organic_ratio()).sum::<f32>() / snapshots.len() as f32
    }

    /// Get total unique wallets seen across all blocks
    pub fn total_unique_wallets(&self) -> u32 {
        self.snapshots.iter().map(|s| s.unique_wallets).sum()
    }

    /// Get block count
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }
}

impl Default for BlockMetricsBuffer {
    fn default() -> Self {
        Self::new(100) // Last 100 blocks (~40 seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_capacity() {
        let mut buffer = BlockMetricsBuffer::new(5);
        for i in 0..10 {
            buffer.push(BlockSnapshot {
                slot: i,
                tx_count: 10,
                unique_wallets: 5,
                bot_tx_count: 3,
                human_tx_count: 7,
                volume_sol: 1.0,
                avg_tx_size: 0.1,
                price_ratio: 1.0,
                timestamp_ms: 0,
            });
        }
        assert_eq!(buffer.len(), 5);
    }

    #[test]
    fn test_bot_ratio_trend() {
        let mut buffer = BlockMetricsBuffer::new(10);
        // First 5 blocks: high bot activity
        for i in 0..5 {
            buffer.push(BlockSnapshot {
                slot: i,
                tx_count: 10,
                bot_tx_count: 8,
                human_tx_count: 2,
                unique_wallets: 3,
                volume_sol: 1.0,
                avg_tx_size: 0.1,
                price_ratio: 1.0,
                timestamp_ms: 0,
            });
        }
        // Next 5 blocks: low bot activity
        for i in 5..10 {
            buffer.push(BlockSnapshot {
                slot: i,
                tx_count: 10,
                bot_tx_count: 2,
                human_tx_count: 8,
                unique_wallets: 8,
                volume_sol: 1.0,
                avg_tx_size: 0.1,
                price_ratio: 1.0,
                timestamp_ms: 0,
            });
        }

        let trend = buffer.bot_ratio_trend(10);
        assert!(trend < 0.0, "Bot ratio should be decreasing, got {}", trend);
    }

    #[test]
    fn test_unique_wallets_trend() {
        let mut buffer = BlockMetricsBuffer::new(10);
        // First 5 blocks: low unique wallets
        for i in 0..5 {
            buffer.push(BlockSnapshot {
                slot: i,
                tx_count: 10,
                bot_tx_count: 5,
                human_tx_count: 5,
                unique_wallets: 3,
                volume_sol: 1.0,
                avg_tx_size: 0.1,
                price_ratio: 1.0,
                timestamp_ms: 0,
            });
        }
        // Next 5 blocks: high unique wallets
        for i in 5..10 {
            buffer.push(BlockSnapshot {
                slot: i,
                tx_count: 10,
                bot_tx_count: 5,
                human_tx_count: 5,
                unique_wallets: 10,
                volume_sol: 1.0,
                avg_tx_size: 0.1,
                price_ratio: 1.0,
                timestamp_ms: 0,
            });
        }

        let trend = buffer.unique_wallets_trend(10);
        assert!(
            trend > 0.0,
            "Unique wallets should be increasing, got {}",
            trend
        );
    }

    #[test]
    fn test_avg_bot_ratio() {
        let mut buffer = BlockMetricsBuffer::new(10);
        // Add blocks with 50% bot ratio
        for i in 0..5 {
            buffer.push(BlockSnapshot {
                slot: i,
                tx_count: 10,
                bot_tx_count: 5,
                human_tx_count: 5,
                unique_wallets: 5,
                volume_sol: 1.0,
                avg_tx_size: 0.1,
                price_ratio: 1.0,
                timestamp_ms: 0,
            });
        }

        let avg = buffer.avg_bot_ratio(5);
        assert!(
            (avg - 0.5).abs() < 0.001,
            "Average bot ratio should be 0.5, got {}",
            avg
        );
    }

    #[test]
    fn test_avg_organic_ratio() {
        let mut buffer = BlockMetricsBuffer::new(10);
        // Add blocks with 70% organic ratio
        for i in 0..5 {
            buffer.push(BlockSnapshot {
                slot: i,
                tx_count: 10,
                bot_tx_count: 3,
                human_tx_count: 7,
                unique_wallets: 5,
                volume_sol: 1.0,
                avg_tx_size: 0.1,
                price_ratio: 1.0,
                timestamp_ms: 0,
            });
        }

        let avg = buffer.avg_organic_ratio(5);
        assert!(
            (avg - 0.7).abs() < 0.001,
            "Average organic ratio should be 0.7, got {}",
            avg
        );
    }

    #[test]
    fn test_block_snapshot_ratios() {
        let snapshot = BlockSnapshot {
            slot: 1,
            tx_count: 10,
            bot_tx_count: 3,
            human_tx_count: 7,
            unique_wallets: 5,
            volume_sol: 1.0,
            avg_tx_size: 0.1,
            price_ratio: 1.0,
            timestamp_ms: 0,
        };

        assert!(
            (snapshot.bot_ratio() - 0.3).abs() < 0.001,
            "Bot ratio should be 0.3"
        );
        assert!(
            (snapshot.organic_ratio() - 0.7).abs() < 0.001,
            "Organic ratio should be 0.7"
        );
    }

    #[test]
    fn test_empty_snapshot_ratios() {
        let snapshot = BlockSnapshot {
            slot: 1,
            tx_count: 0,
            bot_tx_count: 0,
            human_tx_count: 0,
            unique_wallets: 0,
            volume_sol: 0.0,
            avg_tx_size: 0.0,
            price_ratio: 1.0,
            timestamp_ms: 0,
        };

        assert!(
            snapshot.bot_ratio().abs() < 0.001,
            "Bot ratio should be 0.0 for empty snapshot"
        );
        assert!(
            snapshot.organic_ratio().abs() < 0.001,
            "Organic ratio should be 0.0 for empty snapshot"
        );
    }

    #[test]
    fn test_empty_buffer_trends() {
        let buffer = BlockMetricsBuffer::new(10);
        assert!(
            buffer.bot_ratio_trend(5).abs() < 0.001,
            "Empty buffer should have 0 bot trend"
        );
        assert!(
            buffer.unique_wallets_trend(5).abs() < 0.001,
            "Empty buffer should have 0 wallet trend"
        );
        assert!(
            buffer.avg_bot_ratio(5).abs() < 0.001,
            "Empty buffer should have 0 avg bot ratio"
        );
        assert!(
            buffer.avg_organic_ratio(5).abs() < 0.001,
            "Empty buffer should have 0 avg organic ratio"
        );
    }

    #[test]
    fn test_single_snapshot_buffer() {
        let mut buffer = BlockMetricsBuffer::new(10);
        buffer.push(BlockSnapshot {
            slot: 1,
            tx_count: 10,
            bot_tx_count: 5,
            human_tx_count: 5,
            unique_wallets: 5,
            volume_sol: 1.0,
            avg_tx_size: 0.1,
            price_ratio: 1.0,
            timestamp_ms: 0,
        });

        // Single snapshot should have 0 trend (not enough data)
        assert!(
            buffer.bot_ratio_trend(5).abs() < 0.001,
            "Single snapshot should have 0 bot trend"
        );
        assert!(
            buffer.unique_wallets_trend(5).abs() < 0.001,
            "Single snapshot should have 0 wallet trend"
        );

        // But averages should work
        assert!(
            (buffer.avg_bot_ratio(5) - 0.5).abs() < 0.001,
            "Single snapshot avg bot ratio should be 0.5"
        );
    }

    #[test]
    fn test_last_n() {
        let mut buffer = BlockMetricsBuffer::new(10);
        for i in 0..5 {
            buffer.push(BlockSnapshot {
                slot: i,
                tx_count: 10,
                unique_wallets: 5,
                bot_tx_count: 3,
                human_tx_count: 7,
                volume_sol: 1.0,
                avg_tx_size: 0.1,
                price_ratio: 1.0,
                timestamp_ms: 0,
            });
        }

        let last_3 = buffer.last_n(3);
        assert_eq!(last_3.len(), 3);
        // Should be newest first (reversed order)
        assert_eq!(last_3[0].slot, 4);
        assert_eq!(last_3[1].slot, 3);
        assert_eq!(last_3[2].slot, 2);
    }

    #[test]
    fn test_total_unique_wallets() {
        let mut buffer = BlockMetricsBuffer::new(10);
        for i in 0..5 {
            buffer.push(BlockSnapshot {
                slot: i,
                tx_count: 10,
                unique_wallets: 5,
                bot_tx_count: 3,
                human_tx_count: 7,
                volume_sol: 1.0,
                avg_tx_size: 0.1,
                price_ratio: 1.0,
                timestamp_ms: 0,
            });
        }

        assert_eq!(buffer.total_unique_wallets(), 25);
    }

    #[test]
    fn test_is_empty() {
        let buffer = BlockMetricsBuffer::new(10);
        assert!(buffer.is_empty());

        let mut buffer2 = BlockMetricsBuffer::new(10);
        buffer2.push(BlockSnapshot {
            slot: 1,
            tx_count: 10,
            unique_wallets: 5,
            bot_tx_count: 3,
            human_tx_count: 7,
            volume_sol: 1.0,
            avg_tx_size: 0.1,
            price_ratio: 1.0,
            timestamp_ms: 0,
        });
        assert!(!buffer2.is_empty());
    }

    #[test]
    fn test_default_buffer() {
        let buffer = BlockMetricsBuffer::default();
        assert_eq!(buffer.capacity, 100);
        assert!(buffer.is_empty());
    }
}
