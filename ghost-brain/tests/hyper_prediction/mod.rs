//! Test Module for HyperPrediction Oracle
//!
//! This module organizes all integration tests for the HyperPrediction Oracle
//! into separate files by feature area.
//!
//! ## Test Organization
//!
//! - `integration.rs`: End-to-end integration tests and veto conditions
//! - `early_stage.rs`: Patient Observer early stage (tx_count < 2) tests
//! - `scoring.rs`: Score calculation and modifier tests
//! - `signals.rs`: Signal integration tests (LIGMA, QEDD, MCI, etc.)
//! - `praecog.rs`: PRAECOG adversarial analysis tests
//! - `mesa.rs`: MESA microstructure analysis tests
//! - `survivor.rs`: SurvivorScore integration tests
//! - `fre.rs`: Fractal Resonance Engine tests
//! - `task3_integration.rs`: Task 3 formal integration validation tests
//! - `performance.rs`: Performance regression tests
//! - `../config_wiring_test.rs`: Config wiring regression tests (standalone)

pub mod fixtures;
pub mod integration;
pub mod early_stage;
pub mod scoring;
pub mod praecog;
pub mod mesa;
pub mod survivor;
pub mod fre;
pub mod task3_integration;
