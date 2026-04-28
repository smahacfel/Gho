use rustc_hash::{FxHashMap, FxHashSet};
use solana_sdk::pubkey::Pubkey;

/// Observation-time market anomaly tracker (fee spikes + frantic signers).
#[derive(Debug, Clone, Copy, Default)]
pub struct MarketAnomalyOutput {
    pub failed_ratio: f64,
    pub fee_spike: f64,
    pub avg_fee_prev_slot: f64,
    pub current_avg_fee: f64,
    pub frantic_signer_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct MarketAnomalyTx {
    pub slot: Option<u64>,
    pub event_ts_ms: u64,
    pub signer: Pubkey,
    pub success: bool,
    pub priority_fee_micro_lamports: u64,
    pub is_jito_bundle: bool,
}

/// Market anomaly state for slot/fee/frantic monitoring on the observed-time axis.
pub struct MarketAnomalyState {
    failed_tx_count: u32,
    success_tx_count: u32,
    unique_failed_signers: FxHashSet<Pubkey>,
    frantic_signers: FxHashMap<Pubkey, u32>,
    avg_priority_fee_prev_slot: f64,
    current_slot_priority_fee_sum: u64,
    current_slot_tx_count: u32,
    last_bucket_id: u64,
    last_frantic_cleanup_bucket_id: u64,
}

const MARKET_ANOMALY_BUCKET_MS: u64 = 400;

impl MarketAnomalyState {
    pub fn new() -> Self {
        Self {
            failed_tx_count: 0,
            success_tx_count: 0,
            unique_failed_signers: FxHashSet::default(),
            frantic_signers: FxHashMap::default(),
            avg_priority_fee_prev_slot: 0.0,
            current_slot_priority_fee_sum: 0,
            current_slot_tx_count: 0,
            last_bucket_id: 0,
            last_frantic_cleanup_bucket_id: 0,
        }
    }

    pub fn update(&mut self, tx: MarketAnomalyTx) {
        let bucket_id = if tx.event_ts_ms > 0 {
            tx.event_ts_ms / MARKET_ANOMALY_BUCKET_MS.max(1)
        } else {
            // Fallback when observed wall-clock is unavailable
            match tx.slot {
                Some(slot) => slot,
                None => 0,
            }
        };

        if bucket_id > self.last_bucket_id {
            self.avg_priority_fee_prev_slot = if self.current_slot_tx_count > 0 {
                self.current_slot_priority_fee_sum as f64 / self.current_slot_tx_count as f64
            } else {
                0.0
            };
            self.current_slot_priority_fee_sum = 0;
            self.current_slot_tx_count = 0;
            self.last_bucket_id = bucket_id;
        }

        if bucket_id.saturating_sub(self.last_frantic_cleanup_bucket_id) >= 50 {
            self.frantic_signers.clear();
            self.last_frantic_cleanup_bucket_id = bucket_id;
        }

        if !tx.is_jito_bundle {
            self.current_slot_priority_fee_sum = self
                .current_slot_priority_fee_sum
                .saturating_add(tx.priority_fee_micro_lamports);
            self.current_slot_tx_count = self.current_slot_tx_count.saturating_add(1);
        }

        if !tx.success {
            self.failed_tx_count = self.failed_tx_count.saturating_add(1);
            self.unique_failed_signers.insert(tx.signer);
            *self.frantic_signers.entry(tx.signer).or_insert(0) += 1;
        } else {
            self.success_tx_count = self.success_tx_count.saturating_add(1);
        }
    }

    pub fn snapshot(&self) -> MarketAnomalyOutput {
        let total_tx = self.failed_tx_count + self.success_tx_count;
        if total_tx == 0 {
            return MarketAnomalyOutput::default();
        }

        let failed_ratio = self.failed_tx_count as f64 / total_tx as f64;

        let current_avg_fee = if self.current_slot_tx_count > 0 {
            self.current_slot_priority_fee_sum as f64 / self.current_slot_tx_count as f64
        } else {
            0.0
        };

        let fee_spike = if self.avg_priority_fee_prev_slot > 0.0 {
            (current_avg_fee - self.avg_priority_fee_prev_slot) / self.avg_priority_fee_prev_slot
        } else {
            0.0
        };

        let frantic_signer_count = self
            .frantic_signers
            .values()
            .filter(|count| **count > 1)
            .count();

        MarketAnomalyOutput {
            failed_ratio,
            fee_spike: fee_spike.max(0.0),
            avg_fee_prev_slot: self.avg_priority_fee_prev_slot,
            current_avg_fee,
            frantic_signer_count,
        }
    }
}
