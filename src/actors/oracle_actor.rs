//! OracleActor - Actor wrapper for PredictiveOracle
//!
//! This actor wraps the PredictiveOracle component and handles scoring requests.
//! It also integrates the Ghost Intelligence pipeline (Task 12) for post-buy analysis:
//! - DevProfiler: Behavioral analysis of token creators
//! - ClusterHunter: Cabal/Sniper cluster detection
//! - VisionCritic: AI-powered meme quality assessment
//!
//! On TokenBought event, the actor runs all three analyses in parallel using tokio::join!
//! and aggregates results to make exit decisions (Panic Sell vs HODL).

use super::messages::{
    AssessHeldToken, EmergencySell, GetOracleMetrics, OracleMetrics, ScoreCandidate,
    TokenBought, TrailingStopLossMode, UpdateOracleConfig, UpdateStrategy,
};
use crate::oracle::cluster_hunter::{ClusterAnalysis, ClusterHunter, ClusterHunterConfig};
use crate::oracle::profiler::{DevProfile, DevProfiler, DevProfilerConfig};
use crate::oracle::quantum_oracle::{PredictiveOracle, ScoredCandidate, SimpleOracleConfig};
use crate::oracle::scorer::{AggregatedRiskScore, RiskAggregator};
use crate::oracle::vision_critic::{VisionCritic, VisionCriticConfig, VisionCriticResult};
use crate::types::PremintCandidate;
use actix::prelude::*;
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey as SolanaPubkey;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, instrument, warn};

/// Ghost Intelligence configuration for post-buy analysis
#[derive(Debug, Clone)]
pub struct GhostIntelligenceConfig {
    /// DevProfiler configuration
    pub profiler_config: DevProfilerConfig,
    /// ClusterHunter configuration  
    pub cluster_config: ClusterHunterConfig,
    /// VisionCritic configuration
    pub vision_config: VisionCriticConfig,
    /// RPC URL for on-chain analysis
    pub rpc_url: String,
}

impl Default for GhostIntelligenceConfig {
    fn default() -> Self {
        Self {
            profiler_config: DevProfilerConfig::default(),
            cluster_config: ClusterHunterConfig::default(),
            vision_config: VisionCriticConfig::default(),
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        }
    }
}

/// Actor that manages the PredictiveOracle component and Ghost Intelligence
pub struct OracleActor {
    oracle: Arc<Mutex<Option<PredictiveOracle>>>,
    config: Arc<RwLock<SimpleOracleConfig>>,
    candidate_sender: mpsc::Sender<PremintCandidate>,
    scored_receiver: Arc<Mutex<mpsc::Receiver<ScoredCandidate>>>,
    metrics: Arc<RwLock<OracleMetrics>>,
    
    // Ghost Intelligence components
    ghost_config: GhostIntelligenceConfig,
    rpc_client: Arc<RpcClient>,
    http_client: Client,
    
    // Supervisor channel for sending exit decisions
    emergency_sell_sender: Option<mpsc::Sender<EmergencySell>>,
    strategy_update_sender: Option<mpsc::Sender<UpdateStrategy>>,
}

impl OracleActor {
    /// Create a new OracleActor
    pub fn new(
        config: SimpleOracleConfig,
        scored_sender: mpsc::Sender<ScoredCandidate>,
        current_regime: Arc<RwLock<crate::oracle::types::MarketRegime>>,
    ) -> Result<Self, anyhow::Error> {
        Self::with_ghost_intelligence(
            config,
            scored_sender,
            current_regime,
            GhostIntelligenceConfig::default(),
        )
    }
    
    /// Create a new OracleActor with Ghost Intelligence configuration
    pub fn with_ghost_intelligence(
        config: SimpleOracleConfig,
        scored_sender: mpsc::Sender<ScoredCandidate>,
        current_regime: Arc<RwLock<crate::oracle::types::MarketRegime>>,
        ghost_config: GhostIntelligenceConfig,
    ) -> Result<Self, anyhow::Error> {
        use crate::features::store::FeatureStore;
        use std::time::Duration;
        
        let (candidate_sender, candidate_receiver) = mpsc::channel(100);
        let (internal_scored_sender, scored_receiver) = mpsc::channel(100);

        let config_arc = Arc::new(RwLock::new(config.clone()));

        // Create FeatureStore with default configuration
        let feature_store = Arc::new(FeatureStore::new(1000, Duration::from_secs(300)));

        // Create the oracle
        let oracle = PredictiveOracle::new(
            candidate_receiver,
            internal_scored_sender.clone(),
            Arc::clone(&config_arc),
            feature_store,
            current_regime,
        )?;

        let oracle_arc = Arc::new(Mutex::new(Some(oracle)));

        // Spawn a task to forward scored candidates
        let scored_receiver_arc = Arc::new(Mutex::new(scored_receiver));
        let forward_receiver = Arc::clone(&scored_receiver_arc);
        let forward_sender = scored_sender.clone();

        tokio::spawn(async move {
            loop {
                let mut receiver = forward_receiver.lock().await;
                match receiver.recv().await {
                    Some(scored) => {
                        if let Err(e) = forward_sender.send(scored).await {
                            error!("Failed to forward scored candidate: {}", e);
                            break;
                        }
                    }
                    None => {
                        info!("Scored candidate channel closed");
                        break;
                    }
                }
            }
        });
        
        // Create RPC client for on-chain analysis
        let rpc_client = Arc::new(RpcClient::new(ghost_config.rpc_url.clone()));
        let http_client = Client::new();

        Ok(Self {
            oracle: oracle_arc,
            config: config_arc,
            candidate_sender,
            scored_receiver: scored_receiver_arc,
            metrics: Arc::new(RwLock::new(OracleMetrics {
                total_scored: 0,
                avg_scoring_time: 0.0,
                high_score_count: 0,
            })),
            ghost_config,
            rpc_client,
            http_client,
            emergency_sell_sender: None,
            strategy_update_sender: None,
        })
    }
    
    /// Set the emergency sell channel for sending panic sell signals
    pub fn set_emergency_sell_sender(&mut self, sender: mpsc::Sender<EmergencySell>) {
        self.emergency_sell_sender = Some(sender);
    }
    
    /// Set the strategy update channel for sending HODL/strategy signals
    pub fn set_strategy_update_sender(&mut self, sender: mpsc::Sender<UpdateStrategy>) {
        self.strategy_update_sender = Some(sender);
    }
}

impl Actor for OracleActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("OracleActor started with Ghost Intelligence");

        ctx.spawn(
            async move {
                info!("OracleActor: Starting candidate processing loop");

                // The PredictiveOracle will process candidates internally
                // through its own receiver, so we just need to keep the actor alive
            }
            .into_actor(self),
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("OracleActor stopped");
    }
}

// Handle ScoreCandidate messages
impl Handler<ScoreCandidate> for OracleActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    #[tracing::instrument(skip(self, msg, _ctx), fields(mint = %msg.candidate.mint))]
    fn handle(&mut self, msg: ScoreCandidate, _ctx: &mut Context<Self>) -> Self::Result {
        let sender = self.candidate_sender.clone();
        let metrics = Arc::clone(&self.metrics);

        Box::pin(
            async move {
                // Send candidate to oracle for scoring
                sender
                    .send(msg.candidate)
                    .await
                    .map_err(|e| format!("Failed to send candidate to oracle: {}", e))?;

                // Update metrics
                let mut m = metrics.write().await;
                m.total_scored += 1;

                Ok(())
            }
            .into_actor(self),
        )
    }
}

// Handle TokenBought messages - triggers Ghost Intelligence pipeline
// VisionCritic runs in background (SLOW PATH) while DevProfiler and ClusterHunter
// run in parallel (FAST PATH). All three results are aggregated for final decision.
impl Handler<TokenBought> for OracleActor {
    type Result = ResponseActFuture<Self, Result<AggregatedRiskScore, String>>;

    #[tracing::instrument(skip(self, msg, _ctx), fields(mint = %msg.mint, creator = %msg.creator))]
    fn handle(&mut self, msg: TokenBought, _ctx: &mut Context<Self>) -> Self::Result {
        info!("TokenBought event received: mint={}", msg.mint);
        
        let rpc_client = Arc::clone(&self.rpc_client);
        let http_client = self.http_client.clone();
        let ghost_config = self.ghost_config.clone();
        let emergency_sell_sender = self.emergency_sell_sender.clone();
        let strategy_update_sender = self.strategy_update_sender.clone();

        Box::pin(
            async move {
                // Parse pubkeys
                let creator_pubkey = SolanaPubkey::from_str(&msg.creator)
                    .map_err(|e| format!("Invalid creator pubkey: {}", e))?;
                let mint_pubkey = SolanaPubkey::from_str(&msg.mint)
                    .map_err(|e| format!("Invalid mint pubkey: {}", e))?;
                
                // Create analyzers
                let profiler = DevProfiler::new(
                    ghost_config.profiler_config.clone(),
                    Arc::clone(&rpc_client),
                );
                
                let cluster_hunter = ClusterHunter::new(
                    ghost_config.cluster_config.clone(),
                    Arc::clone(&rpc_client),
                );
                
                let vision_critic = VisionCritic::new(
                    ghost_config.vision_config.clone(),
                    http_client.clone(),
                );
                
                // SLOW PATH: Spawn VisionCritic in background (callback logic)
                // This resolves metadata URI and analyzes the image asynchronously
                let mint_for_vision = msg.mint.clone();
                let rpc_for_vision = Arc::clone(&rpc_client);
                let vision_task = tokio::spawn(async move {
                    // Try to resolve metadata URI from on-chain
                    let metadata_uri = resolve_metadata_uri(&mint_for_vision, &rpc_for_vision).await;
                    
                    match metadata_uri {
                        Ok(uri) => {
                            debug!("VisionCritic: Resolved metadata URI for {}: {}", mint_for_vision, uri);
                            vision_critic.analyze_meme_image(&uri).await
                        }
                        Err(e) => {
                            warn!("VisionCritic: Could not resolve metadata URI for {}: {}", mint_for_vision, e);
                            // Return default result if metadata URI cannot be resolved
                            Ok(VisionCriticResult::default())
                        }
                    }
                });
                
                // FAST PATH: Run DevProfiler and ClusterHunter in parallel
                let (profile_result, cluster_result) = tokio::join!(
                    profiler.analyze_creator(creator_pubkey),
                    cluster_hunter.analyze_top_holders(mint_pubkey),
                );
                
                // Wait for VisionCritic background task (SLOW PATH callback)
                let vision_result = vision_task.await
                    .map_err(|e| format!("VisionCritic task panicked: {}", e))?;
                
                // Handle errors with defaults
                let dev_profile = profile_result.unwrap_or_else(|e| {
                    warn!("DevProfiler failed: {}", e);
                    DevProfile::default()
                });
                
                let cluster_analysis = cluster_result.unwrap_or_else(|e| {
                    warn!("ClusterHunter failed: {}", e);
                    ClusterAnalysis::default()
                });
                
                let vision = vision_result.unwrap_or_else(|e| {
                    warn!("VisionCritic failed: {}", e);
                    VisionCriticResult::default()
                });
                
                // Aggregate results from all three analyzers
                let aggregated = RiskAggregator::aggregate(&dev_profile, &cluster_analysis, &vision);
                
                info!(
                    "TokenBought analysis complete: mint={}, risk={:.2}, viral={}, should_panic_sell={}, should_hodl={}",
                    msg.mint, aggregated.risk_score, aggregated.viral_score, 
                    aggregated.should_panic_sell, aggregated.should_hodl
                );
                
                // Process decision and send signals
                if aggregated.should_panic_sell() {
                    let reason = if aggregated.dev_risk > 0.8 {
                        format!("Dev risk {:.2} exceeds threshold", aggregated.dev_risk)
                    } else {
                        format!("Cluster controls {:.1}% supply", aggregated.cluster_controlled_pct)
                    };
                    
                    if let Some(sender) = &emergency_sell_sender {
                        let sell_msg = EmergencySell {
                            mint: msg.mint.clone(),
                            reason,
                            risk_score: aggregated.risk_score,
                        };
                        if let Err(e) = sender.send(sell_msg).await {
                            error!("Failed to send EmergencySell for {}: {}", msg.mint, e);
                        }
                    }
                } else if aggregated.should_hodl() {
                    let reason = format!(
                        "Clean dev + strong viral score ({}/10)",
                        aggregated.viral_score
                    );
                    
                    if let Some(sender) = &strategy_update_sender {
                        let update = UpdateStrategy {
                            mint: msg.mint.clone(),
                            trailing_stop_loss: TrailingStopLossMode::Loose,
                            reason,
                        };
                        if let Err(e) = sender.send(update).await {
                            error!("Failed to send UpdateStrategy for {}: {}", msg.mint, e);
                        }
                    }
                }
                
                Ok(aggregated)
            }
            .into_actor(self),
        )
    }
}

// Handle AssessHeldToken messages - full Ghost Intelligence with metadata
impl Handler<AssessHeldToken> for OracleActor {
    type Result = ResponseActFuture<Self, Result<AggregatedRiskScore, String>>;

    #[tracing::instrument(skip(self, msg, _ctx), fields(mint = %msg.mint, creator = %msg.creator))]
    fn handle(&mut self, msg: AssessHeldToken, _ctx: &mut Context<Self>) -> Self::Result {
        info!("AssessHeldToken request: mint={}", msg.mint);
        
        let rpc_client = Arc::clone(&self.rpc_client);
        let http_client = self.http_client.clone();
        let ghost_config = self.ghost_config.clone();
        let emergency_sell_sender = self.emergency_sell_sender.clone();
        let strategy_update_sender = self.strategy_update_sender.clone();
        let metadata_uri = msg.metadata_uri.clone();

        Box::pin(
            async move {
                // Create analyzers
                let profiler = DevProfiler::new(
                    ghost_config.profiler_config,
                    Arc::clone(&rpc_client),
                );
                
                let cluster_hunter = ClusterHunter::new(
                    ghost_config.cluster_config,
                    Arc::clone(&rpc_client),
                );
                
                let vision_critic = VisionCritic::new(
                    ghost_config.vision_config,
                    http_client,
                );
                
                // Parse pubkeys
                let creator_pubkey = SolanaPubkey::from_str(&msg.creator)
                    .map_err(|e| format!("Invalid creator pubkey: {}", e))?;
                let mint_pubkey = SolanaPubkey::from_str(&msg.mint)
                    .map_err(|e| format!("Invalid mint pubkey: {}", e))?;
                
                // Run all three analyses in parallel using tokio::join!
                let (profile_result, cluster_result, vision_result) = tokio::join!(
                    profiler.analyze_creator(creator_pubkey),
                    cluster_hunter.analyze_top_holders(mint_pubkey),
                    async {
                        if let Some(uri) = &metadata_uri {
                            vision_critic.analyze_meme_image(uri).await
                        } else {
                            Ok(VisionCriticResult::default())
                        }
                    }
                );
                
                // Handle errors with defaults
                let dev_profile = profile_result.unwrap_or_else(|e| {
                    warn!("DevProfiler failed: {}", e);
                    DevProfile::default()
                });
                
                let cluster_analysis = cluster_result.unwrap_or_else(|e| {
                    warn!("ClusterHunter failed: {}", e);
                    ClusterAnalysis::default()
                });
                
                let vision = vision_result.unwrap_or_else(|e| {
                    warn!("VisionCritic failed: {}", e);
                    VisionCriticResult::default()
                });
                
                // Aggregate results
                let aggregated = RiskAggregator::aggregate(&dev_profile, &cluster_analysis, &vision);
                
                info!(
                    "AssessHeldToken complete: mint={}, risk={:.2}, should_panic_sell={}, should_hodl={}",
                    msg.mint, aggregated.risk_score, aggregated.should_panic_sell, aggregated.should_hodl
                );
                
                // Process decision and send signals
                if aggregated.should_panic_sell() {
                    let reason = if aggregated.dev_risk > 0.8 {
                        format!("Dev risk {:.2} exceeds threshold", aggregated.dev_risk)
                    } else {
                        format!("Cluster controls {:.1}% supply", aggregated.cluster_controlled_pct)
                    };
                    
                    if let Some(sender) = &emergency_sell_sender {
                        let sell_msg = EmergencySell {
                            mint: msg.mint.clone(),
                            reason,
                            risk_score: aggregated.risk_score,
                        };
                        if let Err(e) = sender.send(sell_msg).await {
                            error!("Failed to send EmergencySell for {}: {}", msg.mint, e);
                        }
                    }
                } else if aggregated.should_hodl() {
                    let reason = format!(
                        "Clean dev + strong viral score ({}/10)",
                        aggregated.viral_score
                    );
                    
                    if let Some(sender) = &strategy_update_sender {
                        let update = UpdateStrategy {
                            mint: msg.mint.clone(),
                            trailing_stop_loss: TrailingStopLossMode::Loose,
                            reason,
                        };
                        if let Err(e) = sender.send(update).await {
                            error!("Failed to send UpdateStrategy for {}: {}", msg.mint, e);
                        }
                    }
                }
                
                Ok(aggregated)
            }
            .into_actor(self),
        )
    }
}

// Handle UpdateOracleConfig messages
impl Handler<UpdateOracleConfig> for OracleActor {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, msg: UpdateOracleConfig, _ctx: &mut Context<Self>) -> Self::Result {
        let config = Arc::clone(&self.config);

        Box::pin(
            async move {
                let mut cfg = config.write().await;

                if let Some(weights) = msg.weights {
                    cfg.weights = weights;
                    info!("OracleActor: Updated feature weights");
                }

                if let Some(thresholds) = msg.thresholds {
                    cfg.thresholds = thresholds;
                    info!("OracleActor: Updated score thresholds");
                }
            }
            .into_actor(self),
        )
    }
}

// Handle GetOracleMetrics messages
impl Handler<GetOracleMetrics> for OracleActor {
    type Result = ResponseActFuture<Self, OracleMetrics>;

    fn handle(&mut self, _msg: GetOracleMetrics, _ctx: &mut Context<Self>) -> Self::Result {
        let metrics = Arc::clone(&self.metrics);

        Box::pin(async move { metrics.read().await.clone() }.into_actor(self))
    }
}

// ============================================================================
// Helper Functions for Ghost Intelligence
// ============================================================================

/// Metaplex Token Metadata Program ID
const METAPLEX_METADATA_PROGRAM_ID: &str = "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s";

/// Metadata field length limits (Metaplex standard)
const METADATA_NAME_MAX_LEN: usize = 32;
const METADATA_SYMBOL_MAX_LEN: usize = 10;
const METADATA_URI_MAX_LEN: usize = 200;
const METADATA_HEADER_SIZE: usize = 65; // key(1) + update_authority(32) + mint(32)
const METADATA_MIN_SIZE: usize = 100;
const METADATA_FETCH_TIMEOUT_SECS: u64 = 10;

/// Resolve metadata URI from a token mint using Metaplex metadata program.
/// 
/// This function derives the metadata PDA for the given mint and fetches
/// the metadata account to extract the URI field.
async fn resolve_metadata_uri(mint_address: &str, rpc: &RpcClient) -> Result<String, String> {
    use std::time::Duration;
    
    // Parse mint pubkey
    let mint_pubkey = SolanaPubkey::from_str(mint_address)
        .map_err(|e| format!("Invalid mint address: {}", e))?;
    
    // Parse Metaplex program ID
    let metadata_program_id = SolanaPubkey::from_str(METAPLEX_METADATA_PROGRAM_ID)
        .map_err(|e| format!("Invalid metadata program ID: {}", e))?;
    
    // Derive metadata PDA
    // Seeds: ["metadata", metadata_program_id, mint_pubkey]
    let seeds = &[
        b"metadata",
        metadata_program_id.as_ref(),
        mint_pubkey.as_ref(),
    ];
    
    let (metadata_pda, _bump) = SolanaPubkey::find_program_address(seeds, &metadata_program_id);
    
    debug!("Resolving metadata PDA {} for mint {}", metadata_pda, mint_address);
    
    // Fetch metadata account with timeout
    let account = tokio::time::timeout(
        Duration::from_secs(METADATA_FETCH_TIMEOUT_SECS),
        rpc.get_account(&metadata_pda),
    )
    .await
    .map_err(|_| "Timeout fetching metadata account".to_string())?
    .map_err(|e| format!("Failed to fetch metadata account: {}", e))?;
    
    // Extract URI from metadata account data
    extract_uri_from_metadata(&account.data)
}

/// Extract URI from Metaplex metadata account data.
/// 
/// Metadata structure (simplified):
/// - Key (1 byte)
/// - Update authority (32 bytes)
/// - Mint (32 bytes)
/// - Name (4 + max 32 bytes, variable length)
/// - Symbol (4 + max 10 bytes, variable length)
/// - URI (4 + max 200 bytes, variable length)
fn extract_uri_from_metadata(data: &[u8]) -> Result<String, String> {
    if data.len() < METADATA_MIN_SIZE {
        return Err("Metadata account too small".to_string());
    }
    
    // Skip key (1), update authority (32), mint (32) = 65 bytes
    let mut offset = METADATA_HEADER_SIZE;
    
    // Helper to read u32 length prefix safely
    let read_len = |data: &[u8], off: usize| -> Result<usize, String> {
        if off + 4 > data.len() {
            return Err("Invalid metadata: buffer overflow".to_string());
        }
        let bytes: [u8; 4] = data[off..off + 4]
            .try_into()
            .map_err(|_| "Invalid metadata: failed to read length".to_string())?;
        Ok(u32::from_le_bytes(bytes) as usize)
    };
    
    // Skip name (read length, then skip)
    let name_len = read_len(data, offset)?;
    offset += 4 + name_len.min(METADATA_NAME_MAX_LEN);
    
    // Skip symbol (read length, then skip)
    let symbol_len = read_len(data, offset)?;
    offset += 4 + symbol_len.min(METADATA_SYMBOL_MAX_LEN);
    
    // Read URI (length + data)
    let uri_len = read_len(data, offset)?;
    offset += 4;
    
    if uri_len == 0 || uri_len > METADATA_URI_MAX_LEN || offset + uri_len > data.len() {
        return Err("Invalid metadata: URI data out of bounds".to_string());
    }
    
    // Extract URI string, trimming null bytes
    let uri_bytes = &data[offset..offset + uri_len];
    let uri = String::from_utf8_lossy(uri_bytes)
        .trim_matches('\0')
        .trim()
        .to_string();
    
    if uri.is_empty() {
        return Err("Empty URI in metadata".to_string());
    }
    
    Ok(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ghost_intelligence_config_default() {
        let config = GhostIntelligenceConfig::default();
        
        assert_eq!(config.profiler_config.max_signatures, 10);
        assert_eq!(config.cluster_config.top_holders_count, 20);
        assert!(!config.vision_config.enabled);
    }
}
