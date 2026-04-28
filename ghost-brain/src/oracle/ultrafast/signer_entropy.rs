use rustc_hash::FxHashMap;
use solana_sdk::pubkey::Pubkey;

/// Signer entropy tracker for offline/post-decision analysis.
#[derive(Debug, Default)]
pub struct SignerEntropyState {
    signer_counts: FxHashMap<Pubkey, u32>,
    total: u32,
}

impl SignerEntropyState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_signer(&mut self, signer: Pubkey) {
        self.total = self.total.saturating_add(1);
        *self.signer_counts.entry(signer).or_insert(0) += 1;
    }

    pub fn reset(&mut self) {
        self.signer_counts.clear();
        self.total = 0;
    }

    pub fn unique_signers(&self) -> usize {
        self.signer_counts.len()
    }

    pub fn total_count(&self) -> u32 {
        self.total
    }

    pub fn entropy_ratio(&self) -> f64 {
        let total = self.total as f64;
        if total == 0.0 {
            return 0.0;
        }
        let unique = self.signer_counts.len();
        if unique <= 1 {
            return 0.0;
        }
        let entropy = self.entropy();
        (entropy / (unique as f64).ln()).clamp(0.0, 1.0)
    }

    pub fn entropy(&self) -> f64 {
        let total = self.total as f64;
        if total == 0.0 {
            return 0.0;
        }

        let mut entropy = 0.0;
        for count in self.signer_counts.values() {
            let p = *count as f64 / total;
            if p > 0.0 {
                entropy -= p * p.ln();
            }
        }
        entropy
    }
}
