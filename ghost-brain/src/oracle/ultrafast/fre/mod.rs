pub mod engine;
pub mod math;

pub use crate::config::ghost_brain_config::FreConfig;
pub use engine::{FractalAction, FractalEngine, FractalVerdict};
pub use math::{FractalMath, WelfordVariance};
