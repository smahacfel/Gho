use super::observation::{PoolObservationSession, SharedSession};
use crate::config::TxIntelligenceRuntimeConfig;
use crate::tx_intelligence::{CrossPoolVelocityIndex, FundingSourceIndex, TxIntelligenceConfig};
use dashmap::DashMap;
use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::session::types::{SessionId, VerdictOutcome};
use parking_lot::{Mutex, RwLock};
use seer::early_fingerprint::EarlyFingerprintConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub const DEFAULT_MAX_SESSIONS: usize = 10_000;

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub default_observation_duration_ms: u64,
    pub max_sessions: usize,
    pub checkpoint_interval_ms: u64,
    pub tx_intelligence_defaults: TxIntelligenceRuntimeConfig,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            default_observation_duration_ms: 0,
            max_sessions: DEFAULT_MAX_SESSIONS,
            checkpoint_interval_ms: 2_000,
            tx_intelligence_defaults: TxIntelligenceRuntimeConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenSessionRequest {
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub dev_wallet: Option<Pubkey>,
    pub candidate_snapshot: EnhancedCandidate,
    pub created_at_wall_ms: u64,
    pub deadline_wall_ms: Option<u64>,
    pub gatekeeper_config: GatekeeperV2Config,
    pub fingerprint_config: EarlyFingerprintConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionManagerError {
    SessionLimitExceeded { max_sessions: usize },
}

pub struct SessionManager {
    sessions: DashMap<Pubkey, SharedSession>,
    session_counter: AtomicU64,
    config: SessionConfig,
    account_state_core: Arc<AccountStateReducer>,
    cross_pool_velocity_index: Arc<CrossPoolVelocityIndex>,
    funding_source_index: Arc<FundingSourceIndex>,
    lifecycle_lock: Mutex<()>,
}

impl SessionManager {
    #[must_use]
    pub fn new(config: SessionConfig) -> Self {
        Self::new_with_account_state_core(config, Arc::new(AccountStateReducer::new()))
    }

    #[must_use]
    pub fn new_with_account_state_core(
        config: SessionConfig,
        account_state_core: Arc<AccountStateReducer>,
    ) -> Self {
        Self {
            sessions: DashMap::new(),
            session_counter: AtomicU64::new(1),
            config,
            account_state_core,
            cross_pool_velocity_index: Arc::new(CrossPoolVelocityIndex::new()),
            funding_source_index: Arc::new(FundingSourceIndex::new()),
            lifecycle_lock: Mutex::new(()),
        }
    }

    pub fn open_session(
        &self,
        request: OpenSessionRequest,
    ) -> Result<SessionId, SessionManagerError> {
        let _lifecycle_guard = self.lifecycle_lock.lock();
        if let Some(existing) = self.sessions.get(&request.pool_amm_id) {
            return Ok(existing.read().session_id);
        }

        if self.sessions.len() >= self.config.max_sessions {
            ::metrics::increment_counter!(
                "session_manager_open_rejected_total",
                "reason" => "max_sessions"
            );
            return Err(SessionManagerError::SessionLimitExceeded {
                max_sessions: self.config.max_sessions,
            });
        }

        let session_id = SessionId(self.session_counter.fetch_add(1, Ordering::SeqCst));
        let default_window_ms = self
            .config
            .default_observation_duration_ms
            .max(request.gatekeeper_config.max_wait_time_ms);
        let deadline_wall_ms = request
            .deadline_wall_ms
            .unwrap_or_else(|| request.created_at_wall_ms.saturating_add(default_window_ms));
        let tx_intelligence_config = TxIntelligenceConfig::from_gatekeeper_config(
            &request.gatekeeper_config,
            request.fingerprint_config.clone(),
        )
        .apply_runtime_defaults(&self.config.tx_intelligence_defaults);
        let mut session = PoolObservationSession::new_with_account_state_core(
            session_id,
            request.pool_amm_id,
            request.base_mint,
            request.bonding_curve,
            request.dev_wallet,
            request.candidate_snapshot,
            Arc::clone(&self.account_state_core),
            request.created_at_wall_ms,
            deadline_wall_ms,
            &request.gatekeeper_config,
            tx_intelligence_config,
        );
        session.set_cross_pool_velocity_index(Arc::clone(&self.cross_pool_velocity_index));
        session.set_funding_source_index(Arc::clone(&self.funding_source_index));
        session.set_checkpoint_interval_ms(self.config.checkpoint_interval_ms);
        let session = Arc::new(RwLock::new(session));
        self.sessions.insert(request.pool_amm_id, session);
        Ok(session_id)
    }

    #[must_use]
    pub fn get_session(&self, pool_id: &Pubkey) -> Option<SharedSession> {
        self.sessions
            .get(pool_id)
            .map(|entry| Arc::clone(entry.value()))
    }

    pub fn close_session(&self, pool_id: &Pubkey, verdict: VerdictOutcome) -> bool {
        if let Some(session) = self.get_session(pool_id) {
            session.write().apply_verdict(verdict);
            true
        } else {
            false
        }
    }

    pub fn remove_session(&self, pool_id: &Pubkey) -> bool {
        let _lifecycle_guard = self.lifecycle_lock.lock();
        if let Some((_, session)) = self.sessions.remove(pool_id) {
            session.write().close();
            true
        } else {
            false
        }
    }

    #[must_use]
    pub fn active_session_count(&self) -> usize {
        self.sessions.len()
    }

    #[must_use]
    pub fn account_state_core(&self) -> &Arc<AccountStateReducer> {
        &self.account_state_core
    }

    #[must_use]
    pub fn cross_pool_velocity_index(&self) -> Arc<CrossPoolVelocityIndex> {
        Arc::clone(&self.cross_pool_velocity_index)
    }

    #[must_use]
    pub fn funding_source_index(&self) -> Arc<FundingSourceIndex> {
        Arc::clone(&self.funding_source_index)
    }

    pub fn set_funding_stream_available(&self, available: bool) {
        self.funding_source_index.set_stream_available(available);
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new(SessionConfig::default())
    }
}
