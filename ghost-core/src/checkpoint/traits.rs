use super::types::{MaterializedFeatureSet, SessionCheckpoint};
use crate::account_state_core::types::AccountStateFeatures;
use crate::session::types::SessionMetadata;
use crate::tx_intelligence::types::{RiskFlag, TxIntelFeatures};

pub trait CheckpointProducer {
    fn should_checkpoint(&self, now_ms: u64, last_checkpoint_ms: u64) -> bool;

    fn create_checkpoint(
        &mut self,
        account_features: &AccountStateFeatures,
        tx_intel_features: &TxIntelFeatures,
        risk_flags: &[RiskFlag],
    ) -> SessionCheckpoint;
}

pub trait FeatureMaterializer {
    fn materialize(
        &self,
        account_features: AccountStateFeatures,
        tx_intel_features: TxIntelFeatures,
        checkpoints: &[SessionCheckpoint],
        risk_flags: Vec<RiskFlag>,
        metadata: SessionMetadata,
    ) -> MaterializedFeatureSet;
}
