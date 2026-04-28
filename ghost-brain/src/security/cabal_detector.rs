use solana_sdk::pubkey::Pubkey;
use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::fmt;

// =================================================================================
// I. Configuration with Hard Limits (Sanity Checks)
// =================================================================================

#[derive(Debug, Clone)]
pub struct CabalDetectorConfig {
    /// Maksymalny dopuszczalny % podaży w rękach jednego klastra (Funding Cluster).
    /// Przekroczenie = REJECT. (Max 20%)
    pub max_cluster_supply_ratio: f64,

    /// Maksymalny indeks Herfindahla-Hirschmana (HHI).
    /// Powyżej 2500 rynek uznajemy za wysoce skoncentrowany/zmonopolizowany. (Max 5000)
    pub max_hhi_score: f64,

    /// Maksymalny dopuszczalny rozmiar paczki snajperskiej w bloku 0.
    /// Jeśli > 4 portfele kupiły w tym samym slocie i nadal trzymają, to jest Cabal.
    pub max_sniper_bundle_size: usize,

    /// Próg podobieństwa behawioralnego dla sumy podejrzanych grup.
    /// Jeśli suma holderów o powtarzalnych metadanych > 90% = Bot Farm.
    pub behavioral_similarity_threshold: f64,

    /// Głębokość rekursji przy śledzeniu źródła finansowania (aby uniknąć pętli).
    pub max_funding_depth: u8,
}

impl Default for CabalDetectorConfig {
    fn default() -> Self {
        Self {
            max_cluster_supply_ratio: 0.15, // 15%
            max_hhi_score: 2500.0,
            max_sniper_bundle_size: 4,
            behavioral_similarity_threshold: 0.90,
            max_funding_depth: 5,
        }
    }
}

impl CabalDetectorConfig {
    pub fn new(
        max_cluster_supply_ratio: f64,
        max_hhi_score: f64,
        max_sniper_bundle_size: usize,
        behavioral_similarity_threshold: f64,
        max_funding_depth: u8,
    ) -> Self {
        let config = Self {
            max_cluster_supply_ratio,
            max_hhi_score,
            max_sniper_bundle_size,
            behavioral_similarity_threshold,
            max_funding_depth,
        };
        config.validate_sanity_limits();
        config
    }

    /// Hard Constraints Validation / Sanity Checks
    pub fn validate_sanity_limits(&self) {
        if self.max_cluster_supply_ratio > 0.20 {
            panic!("CRITICAL: CabalDetectorConfig: max_cluster_supply_ratio cannot exceed 0.20 (20%)! Attempted: {}", self.max_cluster_supply_ratio);
        }
        if self.max_hhi_score > 5000.0 {
            panic!(
                "CRITICAL: CabalDetectorConfig: max_hhi_score cannot exceed 5000.0! Attempted: {}",
                self.max_hhi_score
            );
        }
        if self.behavioral_similarity_threshold < 0.5 {
            panic!("CRITICAL: CabalDetectorConfig: behavioral_similarity_threshold too low (< 0.5), risks false positives! Attempted: {}", self.behavioral_similarity_threshold);
        }
        if self.max_funding_depth > 10 {
            panic!("CRITICAL: CabalDetectorConfig: max_funding_depth too high (> 10), performance risk! Attempted: {}", self.max_funding_depth);
        }
    }
}

// =================================================================================
// II. Data Structures
// =================================================================================

/// Werdykt systemu scoringowego.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Reject(RejectReason),
}

/// Powód odrzucenia (Raw Types for Zero-Allocation in Hot Path)
#[derive(Debug, PartialEq)]
pub enum RejectReason {
    ClusterDetected {
        size_ratio: f64,
        root_funder: Pubkey,
    },
    HighConcentrationHHI {
        score: f64,
    },
    SniperBundleDetected {
        count: usize,
        slot: u64,
    },
    BotFarmDetected {
        similarity_score: f64,
    },
}

// Implement Display for lazy formatting in logs
impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RejectReason::ClusterDetected {
                size_ratio,
                root_funder,
            } => {
                write!(
                    f,
                    "ClusterDetected: {:.2}% supply controlled by {}",
                    size_ratio * 100.0,
                    root_funder
                )
            }
            RejectReason::HighConcentrationHHI { score } => {
                write!(f, "HighConcentrationHHI: score {:.2}", score)
            }
            RejectReason::SniperBundleDetected { count, slot } => {
                write!(
                    f,
                    "SniperBundleDetected: {} wallets in slot {}",
                    count, slot
                )
            }
            RejectReason::BotFarmDetected { similarity_score } => {
                write!(
                    f,
                    "BotFarmDetected: {:.2}% behavioral similarity",
                    similarity_score * 100.0
                )
            }
        }
    }
}

// Implement Eq manually because f64 doesn't implement Eq
impl Eq for RejectReason {}

/// Profil pojedynczego posiadacza (aktualizowany w czasie rzeczywistym).
#[derive(Debug, Clone)]
pub struct HolderProfile {
    pub address: Pubkey,
    pub balance: u64,
    pub funding_source: Option<Pubkey>,
    pub first_buy_slot: u64,

    // Metadane behawioralne
    pub compute_unit_limit: u32,
    pub priority_fee_lamports: u64,
}

/// Kontekst analityczny dla danego tokena (Snapshot stanu).
pub struct TokenContext {
    pub mint_address: Pubkey,
    pub total_supply: u64,
    pub holders: Vec<HolderProfile>,
    pub known_exchange_addresses: HashSet<Pubkey>,
    pub funding_graph: HashMap<Pubkey, Pubkey>,
}

// =================================================================================
// III. Core Engines Implementation
// =================================================================================

impl TokenContext {
    /// 1. Silnik Analizy Klastrowej (Funding Graph - Memoized)
    /// Wykrywa grupy portfeli finansowane z tego samego źródła.
    pub fn analyze_funding_clusters(
        &self,
        config: &CabalDetectorConfig,
    ) -> Result<(), RejectReason> {
        let mut cluster_map: HashMap<Pubkey, u64> = HashMap::new();
        // Memoization cache: Node -> Root
        let mut root_cache: HashMap<Pubkey, Pubkey> = HashMap::with_capacity(self.holders.len());

        for holder in &self.holders {
            if holder.balance == 0 {
                continue;
            }

            if let Some(direct_funder) = holder.funding_source {
                // Find root using iterative approach with memoization
                let root = self.resolve_root_funder(
                    direct_funder,
                    config.max_funding_depth,
                    &mut root_cache,
                );

                if !self.known_exchange_addresses.contains(&root) {
                    *cluster_map.entry(root).or_insert(0) += holder.balance;
                }
            }
        }

        let supply_f64 = self.total_supply as f64;
        if supply_f64 == 0.0 {
            return Ok(());
        }

        for (root, total_cluster_balance) in cluster_map {
            let ratio = total_cluster_balance as f64 / supply_f64;

            if ratio > config.max_cluster_supply_ratio {
                return Err(RejectReason::ClusterDetected {
                    size_ratio: ratio,
                    root_funder: root,
                });
            }
        }
        Ok(())
    }

    /// Iterative root resolution with memoization and path compression
    fn resolve_root_funder(
        &self,
        start_node: Pubkey,
        max_depth: u8,
        memo: &mut HashMap<Pubkey, Pubkey>,
    ) -> Pubkey {
        // Fast path: check memo
        if let Some(&root) = memo.get(&start_node) {
            return root;
        }

        // If not in graph (and not in exchanges), it is a root itself (or unknown)
        if !self.funding_graph.contains_key(&start_node) {
            memo.insert(start_node, start_node);
            return start_node;
        }

        // Traverse up
        let mut current = start_node;
        let mut path = Vec::with_capacity(max_depth as usize);
        let mut depth = 0;

        loop {
            // Check memo at current step
            if let Some(&known_root) = memo.get(&current) {
                // Path compression: update all previous nodes to point to this known root
                for node in path {
                    memo.insert(node, known_root);
                }
                return known_root;
            }

            path.push(current);

            // Stop if too deep
            if depth >= max_depth {
                break;
            }

            // Move up
            if let Some(&parent) = self.funding_graph.get(&current) {
                // Cycle check: if parent is already in path
                if path.contains(&parent) || parent == current {
                    break;
                }
                // Exchange check
                if self.known_exchange_addresses.contains(&parent) {
                    // Set parent (exchange) as root and break
                    current = parent;
                    break;
                }
                current = parent;
                depth += 1;
            } else {
                // No parent, this is root
                break;
            }
        }

        let root = current;
        // Backfill memo
        for node in path {
            memo.insert(node, root);
        }

        root
    }

    /// 2. Silnik Analizy Synchroniczności (Time-Batching)
    pub fn analyze_time_batching(&self, config: &CabalDetectorConfig) -> Result<(), RejectReason> {
        let mut slot_counts: HashMap<u64, usize> = HashMap::new();

        for holder in &self.holders {
            *slot_counts.entry(holder.first_buy_slot).or_insert(0) += 1;
        }

        for (slot, count) in slot_counts {
            if count >= config.max_sniper_bundle_size {
                return Err(RejectReason::SniperBundleDetected { count, slot });
            }
        }
        Ok(())
    }

    /// 3. Silnik Koncentracji Rynku (HHI)
    pub fn analyze_market_concentration(
        &self,
        config: &CabalDetectorConfig,
    ) -> Result<(), RejectReason> {
        let mut hhi_sum: f64 = 0.0;
        let supply_f64 = self.total_supply as f64;
        if supply_f64 == 0.0 {
            return Ok(());
        }

        for holder in &self.holders {
            let share = (holder.balance as f64 / supply_f64) * 100.0;
            hhi_sum += share * share;
        }

        if hhi_sum > config.max_hhi_score {
            return Err(RejectReason::HighConcentrationHHI { score: hhi_sum });
        }
        Ok(())
    }

    /// 4. Silnik Behawioralny (Fingerprinting - Summing Groups)
    /// Sumuje udział wszystkich grup, które mają powtarzalne wzorce (count >= 2).
    /// Dzięki temu wykrywa Cabal podzielony na grupy A (45%) i B (45%).
    pub fn analyze_behavioral_fingerprints(
        &self,
        config: &CabalDetectorConfig,
    ) -> Result<(), RejectReason> {
        let total_holders = self.holders.len() as f64;
        if total_holders == 0.0 {
            return Ok(());
        }

        let mut pattern_map: HashMap<(u32, u64), usize> = HashMap::new();

        for holder in &self.holders {
            let signature = (holder.compute_unit_limit, holder.priority_fee_lamports);
            *pattern_map.entry(signature).or_insert(0) += 1;
        }

        // Calculate total count of holders belonging to "suspicious groups"
        // A suspicious group is defined as any pattern shared by at least 2 holders (implied repeatable/scripted)
        // For very small holder sets (< 5), this might be aggressive, but for HFT/Launchpad (50+ holders) it works.
        let suspicious_holders_count: usize = pattern_map
            .values()
            .filter(|&&count| count >= 2) // Min group size 2
            .sum();

        let similarity_score = suspicious_holders_count as f64 / total_holders;

        if similarity_score > config.behavioral_similarity_threshold {
            return Err(RejectReason::BotFarmDetected { similarity_score });
        }
        Ok(())
    }
}

// =================================================================================
// IV. The Orchestrator
// =================================================================================

pub struct SecurityEngine {
    config: CabalDetectorConfig,
}

impl SecurityEngine {
    pub fn new(config: CabalDetectorConfig) -> Self {
        Self { config }
    }

    pub fn evaluate_token_security(&self, context: &TokenContext) -> Verdict {
        if let Err(reason) = context.analyze_funding_clusters(&self.config) {
            return Verdict::Reject(reason);
        }
        if let Err(reason) = context.analyze_time_batching(&self.config) {
            return Verdict::Reject(reason);
        }
        if let Err(reason) = context.analyze_behavioral_fingerprints(&self.config) {
            return Verdict::Reject(reason);
        }
        if let Err(reason) = context.analyze_market_concentration(&self.config) {
            return Verdict::Reject(reason);
        }
        Verdict::Approve
    }
}

// =================================================================================
// V. Internal Tests
// =================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    fn mock_pubkey(byte: u8) -> Pubkey {
        let mut bytes = [0u8; 32];
        bytes[0] = byte;
        Pubkey::new_from_array(bytes)
    }

    fn create_test_context(holders_count: usize) -> TokenContext {
        let mut holders = Vec::with_capacity(holders_count);
        for i in 0..holders_count {
            holders.push(HolderProfile {
                address: mock_pubkey(i as u8),
                balance: 1_000,
                funding_source: None,
                first_buy_slot: 100 + i as u64,
                compute_unit_limit: 200_000,
                priority_fee_lamports: 1000,
            });
        }

        TokenContext {
            mint_address: mock_pubkey(255),
            total_supply: (holders_count * 1_000) as u64,
            holders,
            known_exchange_addresses: HashSet::new(),
            funding_graph: HashMap::new(),
        }
    }

    #[test]
    fn test_sanity_check_defaults() {
        let config = CabalDetectorConfig::default();
        config.validate_sanity_limits();
    }

    #[test]
    #[should_panic(expected = "max_cluster_supply_ratio cannot exceed 0.20")]
    fn test_sanity_check_panic_cluster_ratio() {
        let _ = CabalDetectorConfig::new(0.25, 2500.0, 4, 0.9, 5);
    }

    #[test]
    fn test_cluster_detection_with_memoization() {
        let mut context = create_test_context(10);
        let root_funder = mock_pubkey(50);
        let config = CabalDetectorConfig::default();

        context.holders[0].funding_source = Some(root_funder);
        context.holders[1].funding_source = Some(root_funder);
        context.holders[2].funding_source = Some(root_funder);

        let result = context.analyze_funding_clusters(&config);
        match result {
            Err(RejectReason::ClusterDetected {
                size_ratio,
                root_funder: r,
            }) => {
                assert_eq!(r, root_funder);
                assert!((size_ratio - 0.30).abs() < 0.0001); // 30%
            }
            _ => panic!("Should reject due to cluster, got {:?}", result),
        }
    }

    #[test]
    fn test_sniper_bundle_detection() {
        let mut context = create_test_context(10);
        let config = CabalDetectorConfig::default(); // default max 4

        // 4 holders in same slot = Reject
        context.holders[0].first_buy_slot = 500;
        context.holders[1].first_buy_slot = 500;
        context.holders[2].first_buy_slot = 500;
        context.holders[3].first_buy_slot = 500;

        let result = context.analyze_time_batching(&config);
        match result {
            Err(RejectReason::SniperBundleDetected { count, slot }) => {
                assert_eq!(count, 4);
                assert_eq!(slot, 500);
            }
            _ => panic!("Should reject due to bundle, got {:?}", result),
        }
    }

    #[test]
    fn test_bot_fingerprint_summing_groups() {
        let mut context = create_test_context(10);
        let config = CabalDetectorConfig::default(); // Threshold 0.9

        // Scenario: 45% Group A, 45% Group B, 10% Random
        // Total 90% scripted -> Reject

        // Context has 10 holders.
        // 4 holders: Group A
        for i in 0..4 {
            context.holders[i].compute_unit_limit = 100_000;
            context.holders[i].priority_fee_lamports = 100;
        }
        // 4 holders: Group B
        for i in 4..8 {
            context.holders[i].compute_unit_limit = 200_000;
            context.holders[i].priority_fee_lamports = 200;
        }
        // 2 holders: Random (unique) -> Groups of size 1, ignored
        context.holders[8].compute_unit_limit = 300;
        context.holders[9].compute_unit_limit = 400;

        // Suspicious sum = 4 (A) + 4 (B) = 8.
        // 8/10 = 0.8. Default threshold is 0.9. It should PASS (Approve).

        let result_pass = context.analyze_behavioral_fingerprints(&config);
        assert_eq!(result_pass, Ok(()));

        // Now increase Group A to 5 holders.
        // 5 (A) + 4 (B) = 9. 9/10 = 0.9. Still pass (threshold strict > 0.9).
        // Let's make Group A 5 holders, Group B 5 holders (100% scripted).
        context.holders[8].compute_unit_limit = 100_000;
        context.holders[8].priority_fee_lamports = 100;
        context.holders[9].compute_unit_limit = 200_000;
        context.holders[9].priority_fee_lamports = 200;

        // Sum = 5 + 5 = 10. 10/10 = 1.0 > 0.9 -> REJECT.
        let result_fail = context.analyze_behavioral_fingerprints(&config);
        match result_fail {
            Err(RejectReason::BotFarmDetected { similarity_score }) => {
                assert!((similarity_score - 1.0).abs() < 0.0001);
            }
            _ => panic!("Should reject"),
        }
    }

    #[test]
    fn test_funding_recursion_memoization() {
        let mut context = create_test_context(10);
        let config = CabalDetectorConfig::default();

        let root = mock_pubkey(200);
        let mid1 = mock_pubkey(201);
        let mid2 = mock_pubkey(202);

        // Graph: H0 -> Mid1 -> Root
        //        H1 -> Mid2 -> Root
        context.funding_graph.insert(mid1, root);
        context.funding_graph.insert(mid2, root);

        context.holders[0].funding_source = Some(mid1);
        context.holders[1].funding_source = Some(mid2);

        // Balance 1000 each. Total 2000 for Root.
        // 2000 / 10000 = 20% > 15% -> Reject.

        let result = context.analyze_funding_clusters(&config);
        match result {
            Err(RejectReason::ClusterDetected { root_funder: r, .. }) => {
                assert_eq!(r, root);
            }
            _ => panic!("Should detect root funder, result: {:?}", result),
        }
    }
}
