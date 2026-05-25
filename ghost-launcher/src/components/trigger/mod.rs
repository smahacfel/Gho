//! Trigger component and submodules

mod component;
pub mod safety;
pub mod shadow_run;
pub mod tip_guard;

// Re-export main trigger functions and types
pub(crate) use component::BuyBuildProfile;
pub use component::{
    run, run_with_oracle, BuyAccountOverrides, PendingShadowSimulation, PreparedBuyRequest,
    TriggerComponent, TriggerDispatchFailureContext, TriggerDispatchReceipt,
    TriggerPrewarmAdvisory,
};
pub use shadow_run::{ShadowBuySimulationReport, TriggerBuyOutcome};
