use super::traits::CheckpointProducer;
use super::types::{CheckpointTrigger, SessionCheckpoint};
use crate::account_state_core::types::AccountStateFeatures;
use crate::tx_intelligence::types::{RiskFlag, TxIntelFeatures};

#[derive(Debug, Clone, PartialEq)]
pub enum EventCheckpointTrigger {
    DevSell,
    LargeTradeImpact(f64),
    SignerCountMilestone(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointConfig {
    pub interval_ms: u64,
    pub min_tx_between_checkpoints: u64,
    pub enable_event_checkpoints: bool,
    pub event_triggers: Vec<EventCheckpointTrigger>,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            interval_ms: 2_000,
            min_tx_between_checkpoints: 5,
            enable_event_checkpoints: true,
            event_triggers: vec![
                EventCheckpointTrigger::DevSell,
                EventCheckpointTrigger::LargeTradeImpact(25.0),
                EventCheckpointTrigger::SignerCountMilestone(10),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PendingCheckpoint {
    timestamp_ms: u64,
    trigger: CheckpointTrigger,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointEngine {
    pub config: CheckpointConfig,
    pub checkpoint_counter: u32,
    last_checkpoint_tx_count: u64,
    pending_checkpoint: Option<PendingCheckpoint>,
}

impl Default for CheckpointEngine {
    fn default() -> Self {
        Self::new(CheckpointConfig::default())
    }
}

impl CheckpointEngine {
    #[must_use]
    pub fn new(config: CheckpointConfig) -> Self {
        Self {
            config,
            checkpoint_counter: 0,
            last_checkpoint_tx_count: 0,
            pending_checkpoint: None,
        }
    }

    pub fn evaluate_trigger(
        &mut self,
        now_ms: u64,
        last_checkpoint: Option<&SessionCheckpoint>,
        tx_intel_features: &TxIntelFeatures,
        risk_flags: &[RiskFlag],
        latest_trade_price_impact_pct: Option<f64>,
    ) -> Option<CheckpointTrigger> {
        if self.pending_checkpoint.is_some() {
            return self
                .pending_checkpoint
                .as_ref()
                .map(|pending| pending.trigger.clone());
        }

        let last_checkpoint_ms = last_checkpoint.map_or(0, |checkpoint| checkpoint.timestamp_ms);
        let last_checkpoint_tx_count = last_checkpoint
            .map_or(self.last_checkpoint_tx_count, |checkpoint| {
                checkpoint.tx_intel_snapshot.tx_count
            });
        let tx_since_last = tx_intel_features
            .tx_count
            .saturating_sub(last_checkpoint_tx_count);

        let event_trigger = if self.config.enable_event_checkpoints && tx_since_last > 0 {
            self.event_trigger(
                last_checkpoint,
                tx_intel_features,
                risk_flags,
                latest_trade_price_impact_pct,
            )
        } else {
            None
        };

        let trigger = event_trigger.or_else(|| {
            (tx_since_last >= self.config.min_tx_between_checkpoints
                && tx_intel_features.tx_count > 0
                && self.should_checkpoint(now_ms, last_checkpoint_ms))
            .then_some(CheckpointTrigger::TimeBased(now_ms))
        });

        if let Some(trigger) = trigger.clone() {
            self.pending_checkpoint = Some(PendingCheckpoint {
                timestamp_ms: now_ms,
                trigger,
            });
        }

        trigger
    }

    fn event_trigger(
        &self,
        last_checkpoint: Option<&SessionCheckpoint>,
        tx_intel_features: &TxIntelFeatures,
        risk_flags: &[RiskFlag],
        latest_trade_price_impact_pct: Option<f64>,
    ) -> Option<CheckpointTrigger> {
        let last_dev_has_sold = last_checkpoint
            .map(|checkpoint| checkpoint.tx_intel_snapshot.dev_has_sold)
            .unwrap_or(false);
        let last_unique_signers = last_checkpoint
            .map(|checkpoint| checkpoint.tx_intel_snapshot.unique_signers)
            .unwrap_or(0);

        self.config
            .event_triggers
            .iter()
            .find_map(|trigger| match trigger {
                EventCheckpointTrigger::DevSell => {
                    let dev_sell_flag_active = tx_intel_features.dev_has_sold
                        || risk_flags.iter().any(|flag| flag.flag_id == "dev_has_sold");
                    (dev_sell_flag_active && !last_dev_has_sold)
                        .then_some(CheckpointTrigger::EventBased("dev_sell".to_string()))
                }
                EventCheckpointTrigger::LargeTradeImpact(threshold_pct) => {
                    latest_trade_price_impact_pct
                        .filter(|impact_pct| *impact_pct >= *threshold_pct)
                        .map(|impact_pct| {
                            CheckpointTrigger::EventBased(format!(
                                "large_trade_impact:{impact_pct:.4}"
                            ))
                        })
                }
                EventCheckpointTrigger::SignerCountMilestone(milestone) => {
                    (tx_intel_features.unique_signers >= *milestone
                        && last_unique_signers < *milestone)
                        .then_some(CheckpointTrigger::EventBased(format!(
                            "signer_count_milestone:{milestone}"
                        )))
                }
            })
    }
}

impl CheckpointProducer for CheckpointEngine {
    fn should_checkpoint(&self, now_ms: u64, last_checkpoint_ms: u64) -> bool {
        self.config.interval_ms > 0
            && now_ms.saturating_sub(last_checkpoint_ms) >= self.config.interval_ms
    }

    fn create_checkpoint(
        &mut self,
        account_features: &AccountStateFeatures,
        tx_intel_features: &TxIntelFeatures,
        risk_flags: &[RiskFlag],
    ) -> SessionCheckpoint {
        let pending = self.pending_checkpoint.take().unwrap_or(PendingCheckpoint {
            timestamp_ms: 0,
            trigger: CheckpointTrigger::TimeBased(0),
        });
        self.checkpoint_counter = self.checkpoint_counter.saturating_add(1);
        self.last_checkpoint_tx_count = tx_intel_features.tx_count;

        SessionCheckpoint {
            checkpoint_id: self.checkpoint_counter,
            timestamp_ms: pending.timestamp_ms,
            trigger: pending.trigger,
            account_state_snapshot: account_features.clone(),
            tx_intel_snapshot: tx_intel_features.clone(),
            risk_flags: risk_flags.to_vec(),
        }
    }
}
