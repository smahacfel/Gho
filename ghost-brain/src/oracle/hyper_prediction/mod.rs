//! HyperPrediction Oracle - Fast (<2s) token evaluation system.
//! See `ARCHITECTURE.md` for detailed documentation.

// Submodules
pub mod config;
pub mod cyclic_hyper_predictor;
mod orchestrator;
pub mod scoring;
pub mod signals;
pub mod state;
mod utils;
pub mod verdict; // New module

// Re-exports
pub use config::HyperPredictionConfig;
pub use cyclic_hyper_predictor::{CycleResult, CycleScoreRecord, CyclicHyperPredictor};
pub use signals::{SignalBundle, SignalCollector, SignalResult, SignalSource};
pub use state::{AnalysisPhase, HyperPredictionResult, QmanResult, TcfResult};
pub use verdict::{FinalVerdict, OracleDecision, RiskLevel, RiskThresholds};

use crate::analyzers::mesa::MesaAnalyzer;
use crate::chaos::amm_math::AmmPool;
use crate::config::mci_config::MciConfig;
use crate::config::qedd_config::QeddConfig;
use crate::config::TcfConfig;
use crate::fast_pipeline::EnhancedCandidate;
use crate::mci::MciEngine;
use crate::oracle::{
    cluster_hunter::ClusterAnalysis,
    qman::{SignalDetector, TransitionMatrix, UnitaryEvolution},
    scr_extended::SCRExtended,
    second_wave_detector::SecondWaveDetector,
    survivor_score::SurvivorScoreCalculator,
    tcf::TrendCohesionField,
    tx_metrics::TransactionMetrics,
    ultrafast::{fre::FractalEngine, SubSlotMicroentropy},
    ulvf_extended::ULVFExtended,
    wallet_energy_tracker::WalletEnergyTracker,
    HyperOracle, ScoredCandidate,
};
use crate::pumpfun::PumpCurveStateCache;
use crate::qedd::QeddEngine;
use crate::tuning::TunableWeights;
use anyhow::Result;
use parking_lot::Mutex;
use seer::paradox_sensor::ParadoxState;
use tracing::instrument;

const DEFAULT_ACCEPT_THRESHOLD: u8 = 70;

/// HyperPrediction Oracle - Unified sub-2s token evaluation system
#[derive(Clone)]
pub struct HyperPredictionOracle {
    pub(crate) ssmi: SubSlotMicroentropy,
    pub(crate) hyper: HyperOracle,
    pub(crate) mesa_analyzer: MesaAnalyzer,
    pub(crate) scr_extended: SCRExtended,
    pub(crate) ulvf_extended: ULVFExtended,
    pub(crate) qedd: QeddEngine,
    pub(crate) mci: MciEngine,
    pub(crate) wallet_energy_tracker: WalletEnergyTracker,
    pub(crate) transition_matrix: TransitionMatrix,
    pub(crate) unitary_evolution: UnitaryEvolution,
    pub(crate) qman_signal_detector: SignalDetector,
    pub(crate) second_wave_detector: SecondWaveDetector,
    pub(crate) survivor_calculator: SurvivorScoreCalculator,
    pub(crate) fractal_engine: std::sync::Arc<Mutex<FractalEngine>>,
    pub(crate) threshold: u8,
    pub(crate) risk_thresholds: RiskThresholds,
    pub(crate) normalization_config: crate::config::ghost_brain_config::NormalizationConfig,
    pub(crate) ligma_config: crate::config::ghost_brain_config::LigmaConfig,
    pub(crate) fallback_config: crate::config::FallbackConfig,
    pub hyper_prediction_config: HyperPredictionConfig,
    pub(crate) scoring_weights: scoring::ScoringWeights,
    /// TCF (Trend Cohesion Field) configuration
    pub(crate) tcf_config: TcfConfig,
}

impl Default for HyperPredictionOracle {
    fn default() -> Self {
        Self::new(DEFAULT_ACCEPT_THRESHOLD)
    }
}

impl HyperPredictionOracle {
    pub fn new(threshold: u8) -> Self {
        let hyper_prediction_config = HyperPredictionConfig::default();
        Self {
            ssmi: SubSlotMicroentropy::new(),
            hyper: HyperOracle::new(),
            mesa_analyzer: MesaAnalyzer::new(),
            scr_extended: SCRExtended::new(),
            ulvf_extended: ULVFExtended::new(),
            qedd: QeddEngine::new(QeddConfig::default()),
            mci: MciEngine::new(MciConfig::default()),
            wallet_energy_tracker: WalletEnergyTracker::new(),
            transition_matrix: TransitionMatrix::new(),
            unitary_evolution: UnitaryEvolution::new(),
            qman_signal_detector: SignalDetector::new(),
            second_wave_detector: SecondWaveDetector::new(),
            survivor_calculator: SurvivorScoreCalculator::new(),
            fractal_engine: std::sync::Arc::new(Mutex::new(FractalEngine::new(
                crate::config::ghost_brain_config::FreConfig::default(),
            ))),
            threshold,
            risk_thresholds: hyper_prediction_config.risk_thresholds.clone(),
            normalization_config: crate::config::ghost_brain_config::NormalizationConfig::default(),
            ligma_config: crate::config::ghost_brain_config::LigmaConfig::default(),
            fallback_config: crate::config::FallbackConfig::default(),
            hyper_prediction_config,
            scoring_weights: scoring::ScoringWeights::default(),
            tcf_config: TcfConfig::default(),
        }
    }

    pub fn new_with_config(threshold: u8, config: &crate::config::GhostBrainConfig) -> Self {
        let hyper_prediction_config =
            HyperPredictionConfig::from_config(config).unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to load HyperPrediction config: {}, using defaults",
                    e
                );
                HyperPredictionConfig::default()
            });
        Self {
            ssmi: SubSlotMicroentropy::new(),
            hyper: HyperOracle::new(),
            mesa_analyzer: MesaAnalyzer::new(),
            scr_extended: SCRExtended::new(),
            ulvf_extended: ULVFExtended::new(),
            qedd: QeddEngine::new(config.qedd.clone()),
            mci: MciEngine::new(config.mci.clone()),
            wallet_energy_tracker: WalletEnergyTracker::new(),
            transition_matrix: TransitionMatrix::new(),
            unitary_evolution: UnitaryEvolution::new(),
            qman_signal_detector: SignalDetector::new(),
            second_wave_detector: SecondWaveDetector::new(),
            survivor_calculator: SurvivorScoreCalculator::from_ghost_brain_config(config)
                .with_thresholds(hyper_prediction_config.survivor_thresholds.clone())
                .with_risk_multipliers(hyper_prediction_config.risk_multipliers.clone()),
            fractal_engine: std::sync::Arc::new(Mutex::new(FractalEngine::new(config.fre.clone()))),
            threshold,
            risk_thresholds: hyper_prediction_config.risk_thresholds.clone(),
            normalization_config: config.normalization.clone(),
            ligma_config: config.ligma.clone(),
            fallback_config: crate::config::FallbackConfig::default(),
            hyper_prediction_config,
            scoring_weights: scoring::ScoringWeights::from_config(config),
            tcf_config: config.tcf.clone(),
        }
    }

    pub fn threshold(&self) -> u8 {
        self.threshold
    }

    /// Get a reference to the scoring weights used by this oracle
    ///
    /// This allows inspection of the configured weights for testing
    /// and debugging purposes.
    pub fn scoring_weights(&self) -> &scoring::ScoringWeights {
        &self.scoring_weights
    }

    /// Get a reference to the TCF configuration
    ///
    /// This allows inspection of the configured TCF parameters for testing,
    /// debugging, and logging purposes.
    pub fn tcf_config(&self) -> &TcfConfig {
        &self.tcf_config
    }

    /// Create a new TrendCohesionField instance configured with this oracle's settings
    ///
    /// The TCF instance should be created once per pool analysis and reused across
    /// all scoring cycles. It is NOT stored in the oracle because each pool needs
    /// its own independent TCF state.
    pub fn create_tcf(&self) -> TrendCohesionField {
        if self.tcf_config.enabled {
            TrendCohesionField::new()
        } else {
            TrendCohesionField::new()
        }
    }

    #[instrument(skip(self, candidate, pumpfun_cache, explicit_pool_state), fields(pool_id = %candidate.pool_amm_id))]
    pub fn score_candidate(
        &self,
        candidate: &EnhancedCandidate,
        pumpfun_cache: &PumpCurveStateCache,
        explicit_pool_state: Option<&AmmPool>,
        tx_timestamps: Option<&[u64]>,
        tx_data: Option<&[u8]>,
        iwim_result: Option<crate::oracle::ultrafast::IwimResult>,
        chaos_result: Option<crate::chaos::engine::ChaosResult>,
        resonance_result: Option<crate::signals::resonance::ResonanceResult>,
        gene_safety_result: Option<crate::security::gene_mapper::GeneAnalysisResult>,
        hunter_score: Option<u8>,
        tx_metrics: Option<&TransactionMetrics>,
        cluster_result: Option<ClusterAnalysis>,
        paradox_state: Option<ParadoxState>,
        tuned_weights: Option<TunableWeights>,
        ligma_result: Option<crate::signals::LigmaResult>,
    ) -> Result<HyperPredictionResult> {
        orchestrator::score_candidate_impl(
            self,
            candidate,
            pumpfun_cache,
            explicit_pool_state,
            tx_timestamps,
            tx_data,
            iwim_result,
            chaos_result,
            resonance_result,
            gene_safety_result,
            hunter_score,
            tx_metrics,
            cluster_result,
            paradox_state,
            tuned_weights,
            ligma_result,
            None,
            None,
            None,
            None,
            None,
        )
    }

    pub fn score_candidate_with_behavioral(
        &self,
        candidate: &EnhancedCandidate,
        pumpfun_cache: &PumpCurveStateCache,
        explicit_pool_state: Option<&AmmPool>,
        tx_timestamps: Option<&[u64]>,
        tx_data: Option<&[u8]>,
        iwim_result: Option<crate::oracle::ultrafast::IwimResult>,
        chaos_result: Option<crate::chaos::engine::ChaosResult>,
        resonance_result: Option<crate::signals::resonance::ResonanceResult>,
        gene_safety_result: Option<crate::security::gene_mapper::GeneAnalysisResult>,
        hunter_score: Option<u8>,
        tx_metrics: Option<&TransactionMetrics>,
        cluster_result: Option<ClusterAnalysis>,
        paradox_state: Option<ParadoxState>,
        tuned_weights: Option<TunableWeights>,
        ligma_result: Option<crate::signals::LigmaResult>,
        ecto_signal: Option<crate::oracle::ultrafast::EctoSignal>,
        bva_output: Option<crate::oracle::bva::BvaOutput>,
        panic_output: Option<crate::oracle::ultrafast::PanicOutput>,
        tcr_score: Option<crate::oracle::ultrafast::TcrScore>,
        cir_strength: Option<f32>,
    ) -> Result<HyperPredictionResult> {
        orchestrator::score_candidate_impl(
            self,
            candidate,
            pumpfun_cache,
            explicit_pool_state,
            tx_timestamps,
            tx_data,
            iwim_result,
            chaos_result,
            resonance_result,
            gene_safety_result,
            hunter_score,
            tx_metrics,
            cluster_result,
            paradox_state,
            tuned_weights,
            ligma_result,
            ecto_signal,
            bva_output,
            panic_output,
            tcr_score,
            cir_strength,
        )
    }

    pub fn to_scored_candidate(
        &self,
        result: &HyperPredictionResult,
        candidate: &EnhancedCandidate,
    ) -> ScoredCandidate {
        orchestrator::to_scored_candidate_impl(self, result, candidate)
    }
}

// Re-export TCF integration helpers for use in multi-cycle observation
pub use orchestrator::{
    apply_tcf_modulation, build_tcf_observation, compute_tcf_result, interpret_tcf_result,
};
