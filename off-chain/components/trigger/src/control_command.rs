use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AemCommandPriority {
    Default,
    AemPolicy,
    HardSafety,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AemCommandDirective {
    Noop,
    SetTightStop,
    SetLooseStop,
    ForceExitAll,
    ForceExitFractionBps { fraction_bps: u16 },
    FreezePanic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AemCommandApplyResult {
    pub accepted: bool,
    pub reject_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStressSnapshot {
    pub requeue_count: u32,
    pub send_fail_count: u32,
    pub relax_count: u32,
    pub oracle_stale_age_ms: u64,
    pub last_sell_attempt_age_ms: Option<u64>,
}
