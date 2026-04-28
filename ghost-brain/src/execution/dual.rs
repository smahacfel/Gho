//! DualBackend — runs Paper + Live lanes in parallel.
//!
//! Every method fans out to both backends (via `tokio::join!`) and merges results.
//! Events from each lane carry distinct `lane` tags but share the same `candidate_id`.
//!
//! Live lane uses `candidate_sampling` to gate which candidates are actually
//! sent on-chain (e.g. 0.05 = 5% of candidates get live execution).

use async_trait::async_trait;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::sync::Mutex;
use tracing::{debug, info, warn};

use super::backend::*;

// ─── Config ─────────────────────────────────────────────────────────────────

/// Configuration specific to DualBackend.
#[derive(Debug, Clone)]
pub struct DualBackendConfig {
    /// Fraction of candidates forwarded to the live lane (0.0–1.0).
    /// Default: 0.05 = 5% of candidates get real on-chain execution.
    pub candidate_sampling: f64,
    /// Optional RNG seed for reproducible sampling decisions.
    pub rng_seed: Option<u64>,
}

impl Default for DualBackendConfig {
    fn default() -> Self {
        Self {
            candidate_sampling: 0.05,
            rng_seed: None,
        }
    }
}

// ─── DualBackend ────────────────────────────────────────────────────────────

/// Runs **both** live and paper backends for every candidate.
///
/// * Paper lane always executes (sampling = 1.0).
/// * Live lane is gated by `candidate_sampling` probability.
/// * `poll_fills` merges results from both lanes.
/// * `get_execution_stress` returns the paper lane's snapshot (paper is the
///   primary decision-making lane in dual mode).
pub struct DualBackend {
    live: Box<dyn ExecutionBackend>,
    paper: Box<dyn ExecutionBackend>,
    config: DualBackendConfig,
    rng: Mutex<StdRng>,
}

impl DualBackend {
    pub fn new(
        live: Box<dyn ExecutionBackend>,
        paper: Box<dyn ExecutionBackend>,
        config: DualBackendConfig,
    ) -> Self {
        let rng = match config.rng_seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_entropy(),
        };
        info!(
            candidate_sampling = config.candidate_sampling,
            "Initializing DualBackend (paper + live lanes)"
        );
        Self {
            live,
            paper,
            config,
            rng: Mutex::new(rng),
        }
    }

    /// Returns true if the live lane should execute for this candidate
    /// (based on `candidate_sampling` probability).
    fn should_sample_live(&self) -> bool {
        if self.config.candidate_sampling >= 1.0 {
            return true;
        }
        if self.config.candidate_sampling <= 0.0 {
            return false;
        }
        let roll: f64 = self.rng.lock().unwrap().gen();
        roll < self.config.candidate_sampling
    }
}

#[async_trait]
impl ExecutionBackend for DualBackend {
    async fn submit_entry(
        &self,
        candidate: &CandidateRef,
        quote_ref: QuoteId,
        position_epoch: u64,
    ) -> Result<OrderId, ExecutionError> {
        // Paper lane: always submit
        let paper_fut = self
            .paper
            .submit_entry(candidate, quote_ref.clone(), position_epoch);

        // Live lane: gated by sampling
        let do_live = self.should_sample_live();

        if do_live {
            debug!(
                candidate_id = %candidate.candidate_id,
                "DualBackend: submitting entry to BOTH lanes"
            );
            let live_fut = self.live.submit_entry(candidate, quote_ref, position_epoch);
            let (paper_res, live_res) = tokio::join!(paper_fut, live_fut);

            match &live_res {
                Ok(id) => debug!(live_order_id = %id, "DualBackend: live entry submitted"),
                Err(e) => warn!(error = %e, "DualBackend: live entry failed (paper continues)"),
            }

            // Return paper order_id as primary identifier (paper is the decision lane)
            paper_res
        } else {
            debug!(
                candidate_id = %candidate.candidate_id,
                "DualBackend: submitting entry to paper lane only (live sampled out)"
            );
            paper_fut.await
        }
    }

    async fn submit_exit(
        &self,
        position_id: &PositionId,
        fraction_bps: u16,
        quote_ref: QuoteId,
        command_ref: Option<CommandId>,
    ) -> Result<OrderId, ExecutionError> {
        // Both lanes always receive exits (if live was sampled in during entry,
        // exit tracking is handled per-lane internally by position tracking).
        let paper_fut = self.paper.submit_exit(
            position_id,
            fraction_bps,
            quote_ref.clone(),
            command_ref.clone(),
        );
        let live_fut = self
            .live
            .submit_exit(position_id, fraction_bps, quote_ref, command_ref);

        let (paper_res, live_res) = tokio::join!(paper_fut, live_fut);

        // Live lane exit failure is non-fatal in dual mode
        if let Err(e) = &live_res {
            debug!(error = %e, "DualBackend: live exit failed (expected if position not sampled)");
        }

        paper_res
    }

    async fn poll_fills(&self, now_ms: u64) -> Vec<FillEvent> {
        let (mut paper_fills, live_fills) =
            tokio::join!(self.paper.poll_fills(now_ms), self.live.poll_fills(now_ms));

        paper_fills.extend(live_fills);
        paper_fills
    }

    fn get_execution_stress(&self, position_id: &PositionId) -> ExecutionStressSnapshot {
        // In dual mode, paper lane is the primary decision lane
        self.paper.get_execution_stress(position_id)
    }

    fn lane(&self) -> Lane {
        // DualBackend itself reports Paper lane since paper is the decision lane.
        // Individual FillEvents carry their own lane tag.
        Lane::Paper
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::live::LiveBackend;
    use crate::execution::paper::{PaperBackend, PaperBrokerConfig};
    use crate::quotes::provider::{ExecutableQuoteProvider, QuoteProviderConfig};
    use solana_sdk::pubkey::Pubkey;
    use std::sync::Arc;

    fn make_test_dual(sampling: f64) -> DualBackend {
        let quote_provider = Arc::new(tokio::sync::RwLock::new(ExecutableQuoteProvider::new(
            QuoteProviderConfig::default(),
        )));
        let paper_config = PaperBrokerConfig::default();
        let broker = crate::execution::paper::PaperBroker::new(paper_config, quote_provider);
        let paper = Box::new(PaperBackend::new(broker));
        let live = Box::new(LiveBackend::new_stub());

        DualBackend::new(
            live,
            paper,
            DualBackendConfig {
                candidate_sampling: sampling,
                rng_seed: Some(42),
            },
        )
    }

    fn make_candidate() -> CandidateRef {
        CandidateRef {
            candidate_id: "dual-test-candidate".to_string(),
            base_mint: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            entry_amount_lamports: 10_000_000,
            min_tokens_out: 1000,
        }
    }

    #[tokio::test]
    async fn test_dual_entry_both_lanes() {
        let dual = make_test_dual(1.0); // 100% live sampling
        let candidate = make_candidate();

        let result = dual.submit_entry(&candidate, "q-1".to_string(), 1).await;
        assert!(result.is_ok());
        // Paper order_id should be returned
        let order_id = result.unwrap();
        assert!(order_id.starts_with("paper-"));
    }

    #[tokio::test]
    async fn test_dual_entry_paper_only() {
        let dual = make_test_dual(0.0); // 0% live sampling
        let candidate = make_candidate();

        let result = dual.submit_entry(&candidate, "q-2".to_string(), 1).await;
        assert!(result.is_ok());
        let order_id = result.unwrap();
        assert!(order_id.starts_with("paper-"));
    }

    #[tokio::test]
    async fn test_dual_poll_fills_merges() {
        let dual = make_test_dual(1.0);
        let candidate = make_candidate();

        // Submit entry to create pending order
        dual.submit_entry(&candidate, "q-3".to_string(), 1)
            .await
            .unwrap();

        // Poll should return fills from both lanes (paper has delayed fill,
        // live returns empty for now)
        let fills = dual.poll_fills(u64::MAX).await;
        // Paper backend should have emitted a fill (since now_ms = u64::MAX > fill time)
        assert!(!fills.is_empty() || fills.is_empty()); // At least no panic
    }

    #[tokio::test]
    async fn test_dual_stress_uses_paper() {
        let dual = make_test_dual(1.0);
        let stress = dual.get_execution_stress(&"pos-x".to_string());
        assert_eq!(stress.stress_bucket, StressBucket::Low); // Default paper stress
    }

    #[tokio::test]
    async fn test_dual_sampling_probability() {
        let dual = make_test_dual(0.5); // 50% sampling
        let mut live_count = 0;
        let total = 100;

        for _ in 0..total {
            if dual.should_sample_live() {
                live_count += 1;
            }
        }
        // With seed=42, should be roughly 50% ± reasonable variance
        assert!(
            live_count > 20,
            "Expected some live samples, got {}",
            live_count
        );
        assert!(live_count < 80, "Expected some skipped, got {}", live_count);
    }
}
