use solana_client::{
    nonblocking::rpc_client::RpcClient,
};
use solana_sdk::{
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::Mint;
use std::{
    collections::{HashMap, VecDeque},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, RwLock, Semaphore, Mutex},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use anyhow::Result;
use reqwest::Client;
use log::{info, warn, error};
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use nonempty::NonEmpty;
use std::num::NonZeroU32;

// Import types from crate
use crate::types::{PremintCandidate, QuantumCandidateGui};

/// Pump.fun Program ID for bonding curve derivation
const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Number of lamports per SOL for unit conversion
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

// 1. Struktury danych
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredCandidate {
    pub mint: Pubkey,
    pub predicted_score: u8,
    pub feature_scores: HashMap<String, f64>,
    pub reason: String,
    pub timestamp: u64,
    pub calculation_time: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    pub weights: FeatureWeights,
    pub rpc_endpoints: Vec<String>,
    pub pump_fun_api_key: Option<String>,
    pub bitquery_api_key: Option<String>,
    pub thresholds: ScoreThresholds,
    pub rpc_retry_attempts: usize,
    pub rpc_timeout_seconds: u64,
    pub cache_ttl_seconds: u64,
    pub max_parallel_requests: usize,
    pub rate_limit_requests_per_second: u32,
    pub notify_threshold: u8, // GUI notification threshold (default 75)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureWeights {
    pub liquidity: f64,
    pub holder_distribution: f64,
    pub volume_growth: f64,
    pub holder_growth: f64,
    pub price_change: f64,
    pub jito_bundle_presence: f64,
    pub creator_sell_speed: f64,
    pub metadata_quality: f64,
    pub social_activity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreThresholds {
    pub min_liquidity_sol: f64,
    pub whale_threshold: f64,
    pub volume_growth_threshold: f64,
    pub holder_growth_threshold: f64,
    pub min_metadata_quality: f64,
    pub creator_sell_penalty_threshold: u64,
    pub social_activity_threshold: f64,
}

// 2. Główny moduł Oracle
pub struct PredictiveOracle {
    pub candidate_receiver: mpsc::Receiver<PremintCandidate>,
    pub scored_sender: mpsc::Sender<ScoredCandidate>,
    pub gui_suggestions: Arc<Mutex<Option<mpsc::Sender<QuantumCandidateGui>>>>,
    pub rpc_clients: NonEmpty<Arc<RpcClient>>,
    pub http_client: Client,
    pub config: OracleConfig,
    pub token_cache: Arc<RwLock<HashMap<Pubkey, (Instant, TokenData)>>>,
    pub metrics: Arc<RwLock<OracleMetrics>>,
    pub rate_limiter: Arc<DefaultDirectRateLimiter>,
    pub request_semaphore: Arc<Semaphore>,
}

// PredictiveOracle cannot be cloned because mpsc::Receiver is not cloneable
// This is intentional - there should only be one oracle instance receiving candidates

// Helper struct for scoring tasks (contains only cloneable components)
#[derive(Clone)]
struct OracleScorer {
    scored_sender: mpsc::Sender<ScoredCandidate>,
    gui_suggestions: Arc<Mutex<Option<mpsc::Sender<QuantumCandidateGui>>>>,
    #[allow(dead_code)]
    rpc_clients: NonEmpty<Arc<RpcClient>>,
    #[allow(dead_code)]
    http_client: Client,
    config: OracleConfig,
    #[allow(dead_code)]
    token_cache: Arc<RwLock<HashMap<Pubkey, (Instant, TokenData)>>>,
    #[allow(dead_code)]
    metrics: Arc<RwLock<OracleMetrics>>,
    #[allow(dead_code)]
    rate_limiter: Arc<DefaultDirectRateLimiter>,
}

#[derive(Debug, Default, Clone)]
pub struct OracleMetrics {
    pub total_scored: u64,
    pub avg_scoring_time: f64,
    pub high_score_count: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub rpc_errors: u64,
    pub api_errors: u64,
}

#[derive(Debug, Clone)]
pub struct TokenData {
    pub supply: u64,
    pub decimals: u8,
    pub metadata_uri: String,
    pub metadata: Option<Metadata>,
    pub holder_distribution: Vec<HolderData>,
    pub liquidity_pool: Option<LiquidityPool>,
    pub volume_data: VolumeData,
    pub creator_holdings: CreatorHoldings,
    pub holder_history: VecDeque<usize>,
    pub price_history: VecDeque<f64>,
    pub social_activity: SocialActivity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    pub symbol: String,
    pub description: String,
    pub image: String,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute {
    pub trait_type: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct HolderData {
    pub address: Pubkey,
    pub percentage: f64,
    pub is_whale: bool,
}

#[derive(Debug, Clone)]
pub struct LiquidityPool {
    pub sol_amount: f64,
    pub token_amount: f64,
    pub pool_address: Pubkey,
    pub pool_type: PoolType,
}

#[derive(Debug, Clone)]
pub enum PoolType {
    Raydium,
    Orca,
    PumpFun,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct VolumeData {
    pub initial_volume: f64,
    pub current_volume: f64,
    pub volume_growth_rate: f64,
    pub transaction_count: u32,
    pub buy_sell_ratio: f64,
}

#[derive(Debug, Clone)]
pub struct CreatorHoldings {
    pub initial_balance: u64,
    pub current_balance: u64,
    pub first_sell_timestamp: Option<u64>,
    pub sell_transactions: u32,
}

#[derive(Debug, Clone)]
pub struct SocialActivity {
    pub twitter_mentions: u32,
    pub telegram_members: u32,
    pub discord_members: u32,
    pub social_score: f64,
}

// 3. Implementacja Oracle
impl PredictiveOracle {
    pub fn new(
        candidate_receiver: mpsc::Receiver<PremintCandidate>,
        scored_sender: mpsc::Sender<ScoredCandidate>,
        config: OracleConfig,
    ) -> Result<Self> {
        // Validate that rpc_endpoints is not empty and convert to NonEmpty
        let rpc_endpoints_nonempty = NonEmpty::from_vec(config.rpc_endpoints.clone())
            .ok_or_else(|| anyhow::anyhow!("rpc_endpoints cannot be empty"))?;
            
        let rpc_clients = rpc_endpoints_nonempty
            .map(|endpoint| {
                let client = RpcClient::new_with_timeout(
                    endpoint,
                    Duration::from_secs(config.rpc_timeout_seconds)
                );
                Arc::new(client)
            });
        
        let quota = Quota::per_second(NonZeroU32::new(config.rate_limit_requests_per_second)
            .unwrap_or(NonZeroU32::new(10).unwrap()));
        let rate_limiter = Arc::new(RateLimiter::direct(quota));
        
        let request_semaphore = Arc::new(Semaphore::new(config.max_parallel_requests));

        Ok(Self {
            candidate_receiver,
            scored_sender,
            gui_suggestions: Arc::new(Mutex::new(None)),
            rpc_clients,
            http_client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
            config,
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(OracleMetrics::default())),
            rate_limiter,
            request_semaphore,
        })
    }

    pub fn set_gui_sender(&self, sender: mpsc::Sender<QuantumCandidateGui>) {
        tokio::spawn({
            let gui_suggestions = self.gui_suggestions.clone();
            async move {
                let mut gui_lock = gui_suggestions.lock().await;
                *gui_lock = Some(sender);
            }
        });
    }

    pub async fn run(&mut self) {
        info!("Starting Predictive Oracle with {} RPC endpoints", self.rpc_clients.len());
        
        while let Some(candidate) = self.candidate_receiver.recv().await {
            let permit = self.request_semaphore.clone().acquire_owned().await;
            
            // Clone only the needed components for the scoring task
            let scored_sender = self.scored_sender.clone();
            let gui_suggestions = self.gui_suggestions.clone();
            let rpc_clients = self.rpc_clients.clone();
            let http_client = self.http_client.clone();
            let config = self.config.clone();
            let token_cache = self.token_cache.clone();
            let metrics = self.metrics.clone();
            let rate_limiter = self.rate_limiter.clone();
            
            tokio::spawn(async move {
                let start_time = Instant::now();
                
                // Create a temporary scorer for this task
                let scorer = OracleScorer {
                    scored_sender: scored_sender.clone(),
                    gui_suggestions: gui_suggestions.clone(),
                    rpc_clients,
                    http_client,
                    config,
                    token_cache,
                    metrics: metrics.clone(),
                    rate_limiter,
                };
                
                match scorer.score_candidate(&candidate).await {
                    Ok(mut scored) => {
                        let scoring_time = start_time.elapsed().as_micros();
                        scored.calculation_time = scoring_time;
                        
                        // Aktualizuj metryki
                        let mut metrics = metrics.write().await;
                        metrics.total_scored += 1;
                        metrics.avg_scoring_time = 
                            (metrics.avg_scoring_time * (metrics.total_scored - 1) as f64 
                             + scoring_time as f64) / metrics.total_scored as f64;
                        
                        if scored.predicted_score >= 80 {
                            metrics.high_score_count += 1;
                        }
                        drop(metrics);
                        
                        // Send GUI suggestion if score meets threshold
                        if scored.predicted_score >= scorer.config.notify_threshold {
                            let gui_suggestion = QuantumCandidateGui {
                                mint: candidate.mint,
                                score: scored.predicted_score,
                                reason: scored.reason.clone(),
                                feature_scores: scored.feature_scores.clone(),
                                timestamp: candidate.timestamp,
                            };
                            
                            if let Some(sender) = scorer.gui_suggestions.lock().await.as_ref() {
                                if let Err(e) = sender.send(gui_suggestion).await {
                                    warn!("Failed to send GUI suggestion: {}", e);
                                }
                            }
                        }
                        
                        // Wyślij wynik
                        if let Err(e) = scorer.scored_sender.send(scored.clone()).await {
                            error!("Failed to send scored candidate: {}", e);
                        }
                        
                        info!("Scored candidate: {} in {}μs. Score: {}",
                            candidate.mint, scoring_time, scored.predicted_score);
                    }
                    Err(e) => {
                        warn!("Failed to score candidate {}: {}", candidate.mint, e);
                    }
                }
                
                drop(permit);
            });
        }
    }

    // 7. Integracja z GUI
    pub async fn send_to_gui(&self, scored: &ScoredCandidate) {
        let gui_data = json!({
            "mint": scored.mint.to_string(),
            "score": scored.predicted_score,
            "features": scored.feature_scores,
            "reason": scored.reason,
            "calculation_time": scored.calculation_time,
        });
        
        info!("GUI Update: {}", gui_data);
    }

    // 9. Metody utility
    pub async fn get_metrics(&self) -> OracleMetrics {
        (*self.metrics.read().await).clone()
    }
    
    pub async fn clear_cache(&self) {
        let mut cache = self.token_cache.write().await;
        cache.clear();
    }
    
    pub async fn get_cache_size(&self) -> usize {
        let cache = self.token_cache.read().await;
        cache.len()
    }
}

impl OracleScorer {
    async fn score_candidate(&self, candidate: &PremintCandidate) -> Result<ScoredCandidate> {
        let start_time = Instant::now();
        let mut feature_scores = HashMap::new();
        let mut score_components = 0.0;
        let mut reasons = Vec::new();

        // 1. Analiza Jito Bundle (Fastest Check)
        // Jeśli token został utworzony przez Jito Bundle, to znak profesjonalizmu (lub snajpera).
        let is_jito = candidate.is_jito_bundle.unwrap_or(false);
        let jito_score = if is_jito { 100.0 } else { 0.0 };
        feature_scores.insert("jito_bundle_presence".to_string(), jito_score);
        
        if is_jito {
            score_components += 100.0 * self.config.weights.jito_bundle_presence;
            reasons.push("Jito Bundle Detected".to_string());
        }

        // Wybór klienta RPC (Round Robin) - użycie pierwszego jako uproszczenie
        let rpc_client = &self.rpc_clients.head;

        // Parse mint address from string to Pubkey
        let mint_pubkey = match Pubkey::from_str(&candidate.mint) {
            Ok(pk) => pk,
            Err(e) => {
                warn!("Failed to parse mint address {}: {}", candidate.mint, e);
                // Return minimal score if mint address is invalid
                return Ok(ScoredCandidate {
                    mint: Pubkey::default(),
                    predicted_score: 0,
                    feature_scores,
                    reason: format!("Invalid mint address: {}", e),
                    timestamp: candidate.timestamp,
                    calculation_time: start_time.elapsed().as_micros(),
                });
            }
        };

        // 2. Analiza Mint Authority (Security Check)
        // Pobieramy konto mintu, aby sprawdzić, czy dev wyłączył możliwość dodruku (Mint Authority).
        // Jeśli Mint Authority jest aktywne -> RYZYKO (może dodrukować tokeny i zrzucić cenę).
        let authority_score = match rpc_client.get_account(&mint_pubkey).await {
            Ok(account_data) => {
                match Mint::unpack(&account_data.data) {
                    Ok(mint_state) => {
                        if mint_state.mint_authority.is_none() {
                            reasons.push("Mint Authority Revoked (Safe)".to_string());
                            100.0
                        } else {
                            reasons.push("Mint Authority Active (Risk)".to_string());
                            0.0
                        }
                    }
                    Err(e) => {
                        warn!("Failed to unpack mint data for {}: {}", candidate.mint, e);
                        50.0 // Neutral score if we can't verify
                    }
                }
            }
            Err(e) => {
                warn!("Failed to fetch mint account {}: {}", candidate.mint, e);
                50.0 // Neutral score if RPC fails
            }
        };
        
        // Dodajemy to jako 'security' (używamy wagi metadata_quality jako proxy dla security)
        feature_scores.insert("security".to_string(), authority_score);
        score_components += authority_score * self.config.weights.metadata_quality;

        // 3. Analiza Płynności (Liquidity Check)
        // Sprawdzamy ile SOL jest w krzywej bondingowej (lub puli).
        // Derywacja PDA bonding curve (zakładamy Pump.fun).
        let liquidity_score = match self.derive_bonding_curve(&mint_pubkey) {
            Ok(curve_pda) => {
                match rpc_client.get_balance(&curve_pda).await {
                    Ok(balance) => {
                        let liquidity_sol = balance as f64 / LAMPORTS_PER_SOL;
                        
                        let score = if liquidity_sol >= self.config.thresholds.min_liquidity_sol {
                            100.0
                        } else if liquidity_sol > 0.0 {
                            (liquidity_sol / self.config.thresholds.min_liquidity_sol) * 100.0
                        } else {
                            0.0
                        };
                        
                        if liquidity_sol > 0.0 {
                            reasons.push(format!("Liquidity: {:.2} SOL", liquidity_sol));
                        }
                        
                        score
                    }
                    Err(e) => {
                        warn!("Failed to get bonding curve balance for {}: {}", candidate.mint, e);
                        50.0 // Neutral if we can't verify
                    }
                }
            }
            Err(e) => {
                warn!("Failed to derive bonding curve PDA for {}: {}", candidate.mint, e);
                50.0 // Neutral if derivation fails
            }
        };
        
        feature_scores.insert("liquidity".to_string(), liquidity_score);
        score_components += liquidity_score * self.config.weights.liquidity;

        // 4. Normalizacja Wyniku
        // Suma wag powinna wynosić ~1.0. Wynik końcowy to 0-100.
        let total_weight = self.config.weights.jito_bundle_presence 
                         + self.config.weights.metadata_quality 
                         + self.config.weights.liquidity;
        
        let final_score = if total_weight > 0.0 {
            (score_components / total_weight) as u8
        } else {
            0
        };

        // Construct Reason String
        let reason_str = if reasons.is_empty() {
            "Insufficient data".to_string()
        } else {
            reasons.join(", ")
        };

        Ok(ScoredCandidate {
            mint: mint_pubkey,
            predicted_score: final_score.min(100), // Cap at 100
            feature_scores,
            reason: reason_str,
            timestamp: candidate.timestamp,
            calculation_time: start_time.elapsed().as_micros(),
        })
    }

    /// Helper do derywacji adresu bonding curve dla Pump.fun
    fn derive_bonding_curve(&self, mint: &Pubkey) -> Result<Pubkey> {
        let program_id = Pubkey::from_str(PUMP_FUN_PROGRAM_ID)?;
        let seeds: &[&[u8]] = &[b"bonding-curve", mint.as_ref()];
        let (pda, _bump) = Pubkey::find_program_address(seeds, &program_id);
        Ok(pda)
    }
}

// Default implementations
impl Default for FeatureWeights {
    fn default() -> Self {
        Self {
            liquidity: 0.20,
            holder_distribution: 0.15,
            volume_growth: 0.15,
            holder_growth: 0.10,
            price_change: 0.10,
            jito_bundle_presence: 0.05,
            creator_sell_speed: 0.10,
            metadata_quality: 0.10,
            social_activity: 0.05,
        }
    }
}

impl Default for ScoreThresholds {
    fn default() -> Self {
        Self {
            min_liquidity_sol: 10.0,
            whale_threshold: 0.15,
            volume_growth_threshold: 2.0,
            holder_growth_threshold: 1.5,
            min_metadata_quality: 0.7,
            creator_sell_penalty_threshold: 300,
            social_activity_threshold: 100.0,
        }
    }
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            weights: FeatureWeights::default(),
            rpc_endpoints: vec!["https://api.mainnet-beta.solana.com".to_string()],
            pump_fun_api_key: None,
            bitquery_api_key: None,
            thresholds: ScoreThresholds::default(),
            rpc_retry_attempts: 3,
            rpc_timeout_seconds: 10,
            cache_ttl_seconds: 300,
            max_parallel_requests: 10,
            rate_limit_requests_per_second: 20,
            notify_threshold: 75,
        }
    }
}