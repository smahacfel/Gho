pub mod config;
pub mod error;
pub mod feature_adapter;
pub mod hard_safety;
pub mod ledger;
pub mod policy;
pub mod regime;
pub mod regime_book;
pub mod runtime;
pub mod types;

pub use config::{AemConfig, DerivedTimeWindows};
pub use error::AemError;
pub use feature_adapter::{FeatureBuilder, RevolverAemAdapter};
pub use hard_safety::DefaultHardSafetyCheck;
pub use ledger::JsonlAemLedger;
pub use regime::{compute_regime_key, detect_regime};
pub use runtime::{AemRuntime, OutcomeFeatureSource, OutcomeSample};
pub use types::*;

#[cfg(test)]
mod tests;
