use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, info, error, warn};

use super::backend::*;
use crate::events::EventEmitter;
use crate::quotes::provider::ExecutableQuoteProvider;
use crate::trigger::revolver::Revolver;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signer::keypair::Keypair;

pub struct LiveBackendConfig {
    pub rpc_client: Arc<RpcClient>,
    pub payer: Arc<Keypair>,
    pub jito_executor: Option<Arc<crate::jito_bundle::JitoBundleExecutor>>,
    pub enable_jito: bool,
    pub redundancy_factor: usize,
    pub revolver: Arc<RwLock<Revolver>>,
    pub leader_predictor: Option<Arc<crate::leader_predictor::LeaderPredictor>>,
    pub leader_resolver: Option<Arc<dyn trigger::LeaderResolver>>,
    pub leapfrog_config: Option<(usize, bool)>, 
    pub shadow_ledger: Arc<crate::oracle::ShadowLedger>,
    pub metrics: Arc<crate::metrics::E2EMetrics>,
    pub event_emitter: Option<Arc<EventEmitter>>,
    pub post_buy_guardian: Option<Arc<crate::guardian::post_buy::engine::PostBuyGuardian>>,
    pub quote_provider: Arc<RwLock<ExecutableQuoteProvider>>,
    pub snapshot_engine: Arc<crate::oracle::SnapshotEngine>,
    pub quote_max_age_ms: u64,
}

pub struct LiveEntryRequest {
    pub order_id: OrderId,
    pub candidate: CandidateRef,
    pub quote_ref: QuoteId,
    pub position_epoch: u64,
}
