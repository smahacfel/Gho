//! Yellowstone gRPC subscriber for SSOT — wired to real parsers.
//!
//! This module manages account subscriptions and routes incoming raw account
//! data from Yellowstone gRPC to the [`SnapshotStore`]. It replaces the
//! previous stub with real binary parsing via `ghost_core::BondingCurve` for
//! bonding curve accounts and SPL Token account layout for AMM vault accounts.
//!
//! ## Subscription Model
//!
//! - **BondingCurve phase**: subscribes to the bonding curve state account PDA.
//!   Parses `virtual_sol_reserves`, `virtual_token_reserves` from raw account
//!   data via `BondingCurve::from_bytes()`.
//!
//! - **AMM phase**: subscribes to exact AMM vault token accounts by pubkey.
//!   Parses token balance from SPL Token account layout (amount at offset 64).
//!
//! ## Dynamic Subscription Filters
//!
//! `build_account_filters()` generates `SubscribeRequestFilterAccounts` using
//! exact account pubkey lists (not mint-based memcmp), ensuring precise data
//! for AMM reserves in production.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, info, warn};
use yellowstone_grpc_proto::geyser::SubscribeRequestFilterAccounts;

use ghost_core::market_state::BondingCurve;

use super::config::SsotConfig;
use super::snapshot::SnapshotSource;
use super::store::SnapshotStore;

// ─── SPL Token account layout constants ─────────────────────────────────────

/// Minimum size of an SPL Token account (165 bytes).
const SPL_TOKEN_ACCOUNT_SIZE: usize = 165;

/// Offset of the `amount` field (u64 LE) in the SPL Token account layout.
/// Layout: mint (32) + owner (32) + amount (8) = offset 64.
const SPL_TOKEN_AMOUNT_OFFSET: usize = 64;

// ─── Account role for dispatch ──────────────────────────────────────────────

/// Classifies the role of a subscribed account for dispatch routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountRole {
    /// Bonding curve state account PDA.
    BondingCurve,
    /// AMM SOL vault token account.
    AmmSolVault,
    /// AMM token vault token account.
    AmmTokenVault,
}

/// Reverse-lookup entry: maps an account pubkey to its pool context.
#[derive(Debug, Clone)]
struct AccountLookup {
    pool_id: Pubkey,
    base_mint: Pubkey,
    role: AccountRole,
    /// For vault accounts: the pubkey of the sibling vault (O(1) lookup).
    sibling_vault: Option<Pubkey>,
}

/// Cached vault amount with timestamp for TTL-based expiry.
#[derive(Debug, Clone, Copy)]
struct VaultCacheEntry {
    amount: u64,
    ts_ms: u64,
}

/// Default vault cache TTL in milliseconds (3 seconds).
const VAULT_CACHE_TTL_MS: u64 = 3_000;

/// Accounts tracked per pool for Yellowstone subscription.
#[derive(Debug, Clone)]
pub struct PoolSubscription {
    /// Pool identifier.
    pub pool_id: Pubkey,
    /// Base token mint.
    pub base_mint: Pubkey,
    /// Bonding curve state account (PDA) — subscribed during BondingCurve phase.
    pub bonding_curve_account: Option<Pubkey>,
    /// AMM pool state account — subscribed during AMM phase.
    pub amm_pool_account: Option<Pubkey>,
    /// AMM SOL vault account.
    pub amm_sol_vault: Option<Pubkey>,
    /// AMM token vault account.
    pub amm_token_vault: Option<Pubkey>,
}

/// Diagnostic counters for the Yellowstone subscriber hot path.
#[derive(Debug, Default)]
pub struct SubscriberDiagnostics {
    /// Bonding curve updates successfully applied.
    pub curve_updates_applied: AtomicU64,
    /// AMM vault updates successfully applied (both sides resolved).
    pub amm_vault_updates_applied: AtomicU64,
    /// Updates for unknown/unsubscribed pubkeys (ignored).
    pub ignored_updates: AtomicU64,
    /// Bonding curve parse failures.
    pub parse_fail_curve: AtomicU64,
    /// Token account parse failures (too short).
    pub parse_fail_token: AtomicU64,
    /// Unsupported token layouts (Token-2022 / extensions, len > 165).
    pub unsupported_token_layout: AtomicU64,
}

/// Yellowstone subscriber managing account subscriptions for SSOT.
///
/// Tracks which accounts are subscribed per pool and routes incoming
/// account updates to the [`SnapshotStore`].
pub struct YellowstoneSubscriber {
    config: SsotConfig,
    /// Active pool subscriptions.
    subscriptions: Arc<RwLock<Vec<PoolSubscription>>>,
    /// Set of all subscribed account pubkeys (for dedup).
    subscribed_accounts: Arc<RwLock<HashMap<Pubkey, AccountLookup>>>,
    /// Per-vault cached amounts with timestamps. Keyed by vault pubkey.
    /// Used to buffer partial AMM updates until both vaults are known.
    vault_cache: Arc<RwLock<HashMap<Pubkey, VaultCacheEntry>>>,
    /// Reverse map: pool_id → set of subscribed pubkeys (for O(1) cleanup).
    pool_pubkeys: Arc<RwLock<HashMap<Pubkey, Vec<Pubkey>>>>,
    /// Observable diagnostic counters.
    pub diagnostics: Arc<SubscriberDiagnostics>,
}

impl YellowstoneSubscriber {
    /// Create a new subscriber with the given config.
    pub fn new(config: SsotConfig) -> Self {
        Self {
            config,
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            subscribed_accounts: Arc::new(RwLock::new(HashMap::new())),
            vault_cache: Arc::new(RwLock::new(HashMap::new())),
            pool_pubkeys: Arc::new(RwLock::new(HashMap::new())),
            diagnostics: Arc::new(SubscriberDiagnostics::default()),
        }
    }

    /// Register a pool for bonding curve phase subscription.
    ///
    /// Subscribes to the bonding curve state account PDA.
    pub fn subscribe_bonding_curve(
        &self,
        pool_id: Pubkey,
        base_mint: Pubkey,
        bonding_curve_account: Pubkey,
    ) {
        let mut subs = self.subscriptions.write();
        let mut accounts = self.subscribed_accounts.write();

        // Check if already subscribed
        if accounts.contains_key(&bonding_curve_account) {
            debug!(
                pool_id = %pool_id,
                account = %bonding_curve_account,
                "SSOT Yellowstone: bonding curve account already subscribed"
            );
            return;
        }

        accounts.insert(
            bonding_curve_account,
            AccountLookup {
                pool_id,
                base_mint,
                role: AccountRole::BondingCurve,
                sibling_vault: None,
            },
        );
        subs.push(PoolSubscription {
            pool_id,
            base_mint,
            bonding_curve_account: Some(bonding_curve_account),
            amm_pool_account: None,
            amm_sol_vault: None,
            amm_token_vault: None,
        });

        // Track pubkey for O(1) cleanup on unsubscribe
        self.pool_pubkeys
            .write()
            .entry(pool_id)
            .or_default()
            .push(bonding_curve_account);

        info!(
            pool_id = %pool_id,
            base_mint = %base_mint,
            bonding_account = %bonding_curve_account,
            "SSOT Yellowstone: subscribed to bonding curve account"
        );
    }

    /// Register a pool for AMM phase subscription.
    ///
    /// Subscribes to pool state account and token vault accounts.
    pub fn subscribe_amm(
        &self,
        pool_id: Pubkey,
        base_mint: Pubkey,
        amm_pool_account: Pubkey,
        amm_sol_vault: Option<Pubkey>,
        amm_token_vault: Option<Pubkey>,
    ) {
        let mut subs = self.subscriptions.write();
        let mut accounts = self.subscribed_accounts.write();

        // We don't add amm_pool_account as a role here because we only
        // parse vault token accounts for reserves; the pool account itself
        // is tracked in subscriptions for metadata only.
        if let Some(sol_vault) = amm_sol_vault {
            accounts.insert(
                sol_vault,
                AccountLookup {
                    pool_id,
                    base_mint,
                    role: AccountRole::AmmSolVault,
                    sibling_vault: amm_token_vault,
                },
            );
        }
        if let Some(token_vault) = amm_token_vault {
            accounts.insert(
                token_vault,
                AccountLookup {
                    pool_id,
                    base_mint,
                    role: AccountRole::AmmTokenVault,
                    sibling_vault: amm_sol_vault,
                },
            );
        }

        subs.push(PoolSubscription {
            pool_id,
            base_mint,
            bonding_curve_account: None,
            amm_pool_account: Some(amm_pool_account),
            amm_sol_vault,
            amm_token_vault,
        });

        // Track pubkeys for O(1) cleanup on unsubscribe
        {
            let mut pp = self.pool_pubkeys.write();
            let entry = pp.entry(pool_id).or_default();
            if let Some(sv) = amm_sol_vault {
                entry.push(sv);
            }
            if let Some(tv) = amm_token_vault {
                entry.push(tv);
            }
        }

        info!(
            pool_id = %pool_id,
            base_mint = %base_mint,
            amm_account = %amm_pool_account,
            "SSOT Yellowstone: subscribed to AMM pool accounts"
        );
    }

    // ── Raw account data dispatch ───────────────────────────────────────────

    /// Dispatch a raw account update from Yellowstone to the correct handler.
    ///
    /// Looks up `account_pubkey` in the reverse-lookup map to determine whether
    /// it is a bonding curve state account or an AMM vault token account, then
    /// parses the raw `data` accordingly and updates `store`.
    ///
    /// Returns `true` if the update was successfully routed and parsed.
    pub fn on_raw_account_update(
        &self,
        store: &SnapshotStore,
        account_pubkey: &Pubkey,
        data: &[u8],
    ) -> bool {
        let lookup = {
            let accounts = self.subscribed_accounts.read();
            accounts.get(account_pubkey).cloned()
        };

        let lookup = match lookup {
            Some(l) => l,
            None => {
                debug!(
                    account = %account_pubkey,
                    "SSOT Yellowstone: unknown account, ignoring update"
                );
                self.diagnostics
                    .ignored_updates
                    .fetch_add(1, Ordering::Relaxed);
                return false;
            }
        };

        match lookup.role {
            AccountRole::BondingCurve => self.handle_bonding_curve_update(store, &lookup, data),
            AccountRole::AmmSolVault | AccountRole::AmmTokenVault => {
                self.handle_amm_vault_update(store, account_pubkey, &lookup, data)
            }
        }
    }

    /// Parse bonding curve raw account data and update the store.
    fn handle_bonding_curve_update(
        &self,
        store: &SnapshotStore,
        lookup: &AccountLookup,
        data: &[u8],
    ) -> bool {
        // Gate: data length must exactly match expected BondingCurve struct size.
        // Non-curve program accounts (e.g. ~145 bytes) must be rejected.
        let expected_len = std::mem::size_of::<BondingCurve>();
        if data.len() != expected_len {
            debug!(
                pool_id = %lookup.pool_id,
                data_len = data.len(),
                expected_len = expected_len,
                "SSOT Yellowstone: bonding curve data length mismatch, ignoring"
            );
            self.diagnostics
                .parse_fail_curve
                .fetch_add(1, Ordering::Relaxed);
            return false;
        }

        match parse_bonding_curve_raw(data) {
            Some((v_tokens, v_sol)) => {
                store.update_bonding(
                    lookup.pool_id,
                    lookup.base_mint,
                    v_sol,
                    v_tokens,
                    None,
                    SnapshotSource::Yellowstone,
                );
                self.diagnostics
                    .curve_updates_applied
                    .fetch_add(1, Ordering::Relaxed);
                true
            }
            None => {
                debug!(
                    pool_id = %lookup.pool_id,
                    data_len = data.len(),
                    "SSOT Yellowstone: failed to parse bonding curve account data"
                );
                self.diagnostics
                    .parse_fail_curve
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Parse AMM vault token account data and update the store.
    ///
    /// Uses O(1) sibling lookup via `AccountLookup::sibling_vault`.
    /// Caches the parsed amount per vault pubkey with TTL. Only pushes to the
    /// store once both SOL and token vault amounts are known and fresh.
    fn handle_amm_vault_update(
        &self,
        store: &SnapshotStore,
        account_pubkey: &Pubkey,
        lookup: &AccountLookup,
        data: &[u8],
    ) -> bool {
        let amount = match parse_token_account_amount(data) {
            Ok(a) => a,
            Err(TokenParseError::TooShort) => {
                warn!(
                    pool_id = %lookup.pool_id,
                    role = ?lookup.role,
                    data_len = data.len(),
                    "SSOT Yellowstone: SPL token account data too short"
                );
                self.diagnostics
                    .parse_fail_token
                    .fetch_add(1, Ordering::Relaxed);
                return false;
            }
            Err(TokenParseError::UnsupportedLayout) => {
                debug!(
                    pool_id = %lookup.pool_id,
                    role = ?lookup.role,
                    data_len = data.len(),
                    "SSOT Yellowstone: unsupported token layout (Token-2022/extensions), skipping"
                );
                self.diagnostics
                    .unsupported_token_layout
                    .fetch_add(1, Ordering::Relaxed);
                return false;
            }
        };

        let now_ms = Self::now_ms();

        // Cache the current vault amount with timestamp.
        {
            let mut cache = self.vault_cache.write();
            cache.insert(
                *account_pubkey,
                VaultCacheEntry {
                    amount,
                    ts_ms: now_ms,
                },
            );
        }

        // O(1) sibling lookup via stored sibling_vault field.
        // Guard: AMM update only when both sides are fresh in cache.
        // No fallback to stale snapshot — prevents "mixed freshness" pricing
        // where one side is fresh and the other comes from an old snapshot.
        let sibling_amount = lookup.sibling_vault.and_then(|sib_pk| {
            let cache = self.vault_cache.read();
            cache.get(&sib_pk).and_then(|entry| {
                // TTL check: only use fresh sibling data
                if now_ms.saturating_sub(entry.ts_ms) <= VAULT_CACHE_TTL_MS {
                    Some(entry.amount)
                } else {
                    None
                }
            })
        });

        // Construct reserves — sibling must be fresh (no fallback to store)
        let (reserve_sol, reserve_token) = match lookup.role {
            AccountRole::AmmSolVault => (amount, sibling_amount.unwrap_or(0)),
            AccountRole::AmmTokenVault => (sibling_amount.unwrap_or(0), amount),
            _ => unreachable!(),
        };

        // Only push when both sides are nonzero — both must be fresh in cache.
        if reserve_sol == 0 || reserve_token == 0 {
            debug!(
                pool_id = %lookup.pool_id,
                reserve_sol = reserve_sol,
                reserve_token = reserve_token,
                "SSOT Yellowstone: partial AMM vault update cached, waiting for sibling"
            );
            return true; // cached successfully, but not yet pushed
        }

        let fee_bps = store.get(&lookup.pool_id).and_then(|s| s.fee_bps);

        store.update_amm(
            lookup.pool_id,
            lookup.base_mint,
            reserve_sol,
            reserve_token,
            fee_bps,
            SnapshotSource::Yellowstone,
        );
        self.diagnostics
            .amm_vault_updates_applied
            .fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Remove all subscriptions and caches for a pool.
    ///
    /// Removes the pool from `subscriptions`, `subscribed_accounts`,
    /// `vault_cache`, and `pool_pubkeys`. Also removes the pool's snapshot
    /// from `store` so downstream consumers (AEM / Manager) cannot price
    /// off stale data.
    ///
    /// If no `SnapshotStore` reference is available at the call site, pass
    /// `None` — the subscriber-side cleanup will still happen.
    pub fn unsubscribe_pool(&self, pool_id: &Pubkey, store: Option<&SnapshotStore>) {
        // Remove pubkeys from subscribed_accounts and vault_cache
        let pubkeys = self.pool_pubkeys.write().remove(pool_id);
        if let Some(pks) = pubkeys {
            let mut accounts = self.subscribed_accounts.write();
            let mut cache = self.vault_cache.write();
            for pk in &pks {
                accounts.remove(pk);
                cache.remove(pk);
            }
        }

        // Remove from subscriptions vec
        self.subscriptions.write().retain(|s| s.pool_id != *pool_id);

        // SSOT invariant: snapshot must not outlive the subscription.
        if let Some(s) = store {
            s.remove(pool_id);
        }

        info!(
            pool_id = %pool_id,
            "SSOT Yellowstone: unsubscribed pool (subscriber + store cleaned)"
        );
    }

    /// Evict expired entries from the vault cache.
    ///
    /// Call periodically (e.g. every few seconds) to prevent unbounded growth.
    pub fn gc_vault_cache(&self) {
        let now_ms = Self::now_ms();
        let mut cache = self.vault_cache.write();
        cache.retain(|_, entry| now_ms.saturating_sub(entry.ts_ms) <= VAULT_CACHE_TTL_MS);
    }

    /// Spawn a background tokio task that calls [`gc_vault_cache()`] every
    /// `interval` seconds. Returns a `JoinHandle` that can be aborted on
    /// shutdown.
    ///
    /// # Example (in seer / gRPC runtime)
    /// ```ignore
    /// let gc_handle = subscriber.spawn_gc_task(2);
    /// // … on shutdown:
    /// gc_handle.abort();
    /// ```
    pub fn spawn_gc_task(self: &Arc<Self>, interval_secs: u64) -> tokio::task::JoinHandle<()> {
        let sub = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                tick.tick().await;
                sub.gc_vault_cache();
            }
        })
    }

    // TODO(perf): Replace SystemTime::now() with monotonic Instant-based
    // coarse timer (e.g. cached once per tick) to avoid wall-clock jumps
    // from NTP/VPS and reduce syscall overhead on the hot path.
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    // ── Pre-parsed convenience methods (kept for backward compat) ───────────

    /// Process a bonding curve account update from Yellowstone.
    ///
    /// Accepts pre-parsed virtual reserves and updates the store.
    pub fn on_bonding_curve_account_update(
        &self,
        store: &SnapshotStore,
        pool_id: Pubkey,
        base_mint: Pubkey,
        v_sol: u64,
        v_tokens: u64,
        market_cap_sol: Option<f64>,
    ) {
        store.update_bonding(
            pool_id,
            base_mint,
            v_sol,
            v_tokens,
            market_cap_sol,
            SnapshotSource::Yellowstone,
        );
    }

    /// Process an AMM account update from Yellowstone.
    ///
    /// Accepts pre-parsed reserve balances and updates the store.
    pub fn on_amm_account_update(
        &self,
        store: &SnapshotStore,
        pool_id: Pubkey,
        base_mint: Pubkey,
        reserve_sol: u64,
        reserve_token: u64,
        fee_bps: Option<u16>,
    ) {
        store.update_amm(
            pool_id,
            base_mint,
            reserve_sol,
            reserve_token,
            fee_bps,
            SnapshotSource::Yellowstone,
        );
    }

    // ── Subscription filter generation ──────────────────────────────────────

    /// Build `SubscribeRequestFilterAccounts` using exact account pubkeys.
    ///
    /// Returns a map of filter-label → filter, suitable for inclusion in a
    /// `SubscribeRequest.accounts`. Uses exact `account` lists (not
    /// mint-memcmp) for precise AMM vault subscription in production.
    pub fn build_account_filters(&self) -> HashMap<String, SubscribeRequestFilterAccounts> {
        let accounts = self.subscribed_accounts.read();

        let mut bonding_keys: Vec<String> = Vec::new();
        let mut amm_vault_keys: Vec<String> = Vec::new();

        for (pubkey, lookup) in accounts.iter() {
            match lookup.role {
                AccountRole::BondingCurve => {
                    bonding_keys.push(pubkey.to_string());
                }
                AccountRole::AmmSolVault | AccountRole::AmmTokenVault => {
                    amm_vault_keys.push(pubkey.to_string());
                }
            }
        }

        let mut filters = HashMap::new();

        if !bonding_keys.is_empty() {
            filters.insert(
                "ssot_bonding_curves".to_string(),
                SubscribeRequestFilterAccounts {
                    account: bonding_keys,
                    owner: vec![],
                    filters: vec![],
                },
            );
        }

        if !amm_vault_keys.is_empty() {
            filters.insert(
                "ssot_amm_vaults".to_string(),
                SubscribeRequestFilterAccounts {
                    account: amm_vault_keys,
                    owner: vec![],
                    filters: vec![],
                },
            );
        }

        filters
    }

    // ── Accessors ───────────────────────────────────────────────────────────

    /// Get the set of all currently subscribed account pubkeys.
    pub fn subscribed_accounts(&self) -> Vec<Pubkey> {
        self.subscribed_accounts.read().keys().copied().collect()
    }

    /// Get the number of active pool subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.read().len()
    }

    /// Whether Yellowstone is enabled in config.
    pub fn is_enabled(&self) -> bool {
        self.config.enable_yellowstone
    }
}

// ─── Binary parsers ─────────────────────────────────────────────────────────

/// Parse raw bonding curve account data using `ghost_core::BondingCurve`.
///
/// Returns `(virtual_token_reserves, virtual_sol_reserves)` on success.
pub fn parse_bonding_curve_raw(data: &[u8]) -> Option<(u64, u64)> {
    let expected = std::mem::size_of::<BondingCurve>();
    if data.len() != expected {
        return None;
    }
    let bc = BondingCurve::from_bytes(data)?;
    Some((bc.virtual_token_reserves, bc.virtual_sol_reserves))
}

/// Error types for SPL Token account parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenParseError {
    /// Account data is too short (< 165 bytes).
    TooShort,
    /// Account data has unsupported layout (Token-2022 / extensions, > 165 bytes).
    UnsupportedLayout,
}

/// Parse SPL Token account data and extract the `amount` field.
///
/// SPL Token account layout (exactly 165 bytes):
///   - [0..32]   mint (Pubkey)
///   - [32..64]  owner (Pubkey)
///   - [64..72]  amount (u64, little-endian)
///   - [72..]    state, delegate, etc.
///
/// Returns `Ok(amount)` for standard SPL Token accounts (165 bytes).
/// Returns `Err(TooShort)` if data is smaller than 165 bytes.
/// Returns `Err(UnsupportedLayout)` if data is larger than 165 bytes
/// (Token-2022 with extensions).
pub fn parse_token_account_amount(data: &[u8]) -> Result<u64, TokenParseError> {
    if data.len() < SPL_TOKEN_ACCOUNT_SIZE {
        return Err(TokenParseError::TooShort);
    }
    if data.len() > SPL_TOKEN_ACCOUNT_SIZE {
        return Err(TokenParseError::UnsupportedLayout);
    }
    let bytes: [u8; 8] = data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
        .try_into()
        .map_err(|_| TokenParseError::TooShort)?;
    Ok(u64::from_le_bytes(bytes))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SsotConfig {
        SsotConfig {
            enable_yellowstone: true,
            yellowstone_endpoint: "http://localhost:10000".to_string(),
            ..SsotConfig::default()
        }
    }

    // ── Subscription tests ──────────────────────────────────────────────

    #[test]
    fn test_subscribe_bonding_curve() {
        let sub = YellowstoneSubscriber::new(test_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bc_account = Pubkey::new_unique();

        sub.subscribe_bonding_curve(pool, mint, bc_account);

        assert_eq!(sub.subscription_count(), 1);
        assert!(sub.subscribed_accounts().contains(&bc_account));
    }

    #[test]
    fn test_subscribe_amm() {
        let sub = YellowstoneSubscriber::new(test_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let amm_acc = Pubkey::new_unique();
        let sol_vault = Pubkey::new_unique();
        let tok_vault = Pubkey::new_unique();

        sub.subscribe_amm(pool, mint, amm_acc, Some(sol_vault), Some(tok_vault));

        assert_eq!(sub.subscription_count(), 1);
        let accounts = sub.subscribed_accounts();
        assert!(accounts.contains(&sol_vault));
        assert!(accounts.contains(&tok_vault));
    }

    #[test]
    fn test_dedup_bonding_subscription() {
        let sub = YellowstoneSubscriber::new(test_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bc_account = Pubkey::new_unique();

        sub.subscribe_bonding_curve(pool, mint, bc_account);
        sub.subscribe_bonding_curve(pool, mint, bc_account);

        // Only 1 subscription because of dedup
        assert_eq!(sub.subscription_count(), 1);
    }

    // ── Pre-parsed convenience (backward compat) ────────────────────────

    #[test]
    fn test_on_bonding_curve_update_routes_to_store() {
        let sub = YellowstoneSubscriber::new(test_config());
        let store = SnapshotStore::new(SsotConfig::default());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        sub.on_bonding_curve_account_update(
            &store,
            pool,
            mint,
            30_000_000_000,
            1_073_000_000_000_000,
            None,
        );

        let snap = store.get(&pool).expect("snapshot should exist");
        assert_eq!(snap.v_sol, Some(30_000_000_000));
    }

    #[test]
    fn test_on_amm_update_routes_to_store() {
        let sub = YellowstoneSubscriber::new(test_config());
        let store = SnapshotStore::new(SsotConfig::default());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        sub.on_amm_account_update(&store, pool, mint, 50_000_000_000, 200_000_000, Some(25));

        let snap = store.get(&pool).expect("snapshot should exist");
        assert_eq!(snap.reserve_sol, Some(50_000_000_000));
        assert_eq!(snap.fee_bps, Some(25));
    }

    // ── Raw binary parsing tests ────────────────────────────────────────

    #[test]
    fn test_parse_bonding_curve_raw_valid() {
        // Build valid BondingCurve bytes (56 bytes, #[repr(C)])
        let mut data = vec![0u8; std::mem::size_of::<BondingCurve>()];
        // discriminator at offset 0 (8 bytes) — any value
        data[0..8].copy_from_slice(&0xDEAD_BEEF_u64.to_le_bytes());
        // virtual_token_reserves at offset 8
        data[8..16].copy_from_slice(&1_000_000_000_000_u64.to_le_bytes());
        // virtual_sol_reserves at offset 16
        data[16..24].copy_from_slice(&30_000_000_000_u64.to_le_bytes());

        let result = parse_bonding_curve_raw(&data);
        assert!(result.is_some());
        let (v_tokens, v_sol) = result.unwrap();
        assert_eq!(v_tokens, 1_000_000_000_000);
        assert_eq!(v_sol, 30_000_000_000);
    }

    #[test]
    fn test_parse_bonding_curve_raw_too_short() {
        let data = vec![0u8; 20];
        assert!(parse_bonding_curve_raw(&data).is_none());
    }

    #[test]
    fn test_parse_token_account_amount_valid() {
        let mut data = vec![0u8; SPL_TOKEN_ACCOUNT_SIZE];
        let amount: u64 = 42_000_000_000;
        data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
            .copy_from_slice(&amount.to_le_bytes());

        assert_eq!(parse_token_account_amount(&data), Ok(amount));
    }

    #[test]
    fn test_parse_token_account_amount_too_short() {
        let data = vec![0u8; 50];
        assert_eq!(
            parse_token_account_amount(&data),
            Err(TokenParseError::TooShort)
        );
    }

    #[test]
    fn test_parse_token_account_unsupported_layout() {
        // Token-2022 with extensions: > 165 bytes
        let data = vec![0u8; 200];
        assert_eq!(
            parse_token_account_amount(&data),
            Err(TokenParseError::UnsupportedLayout)
        );
    }

    // ── Raw dispatch tests ──────────────────────────────────────────────

    #[test]
    fn test_on_raw_account_update_bonding_curve() {
        let sub = YellowstoneSubscriber::new(test_config());
        let store = SnapshotStore::new(SsotConfig::default());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bc_account = Pubkey::new_unique();

        sub.subscribe_bonding_curve(pool, mint, bc_account);

        // Build valid BondingCurve bytes
        let mut data = vec![0u8; std::mem::size_of::<BondingCurve>()];
        data[0..8].copy_from_slice(&0x1234_u64.to_le_bytes());
        data[8..16].copy_from_slice(&500_000_000_000_u64.to_le_bytes()); // v_tokens
        data[16..24].copy_from_slice(&20_000_000_000_u64.to_le_bytes()); // v_sol

        assert!(sub.on_raw_account_update(&store, &bc_account, &data));

        let snap = store.get(&pool).expect("snapshot should exist");
        assert_eq!(snap.v_sol, Some(20_000_000_000));
        assert_eq!(snap.v_tokens, Some(500_000_000_000));
    }

    #[test]
    fn test_on_raw_account_update_amm_vaults() {
        let sub = YellowstoneSubscriber::new(test_config());
        let store = SnapshotStore::new(SsotConfig::default());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let amm_acc = Pubkey::new_unique();
        let sol_vault = Pubkey::new_unique();
        let tok_vault = Pubkey::new_unique();

        sub.subscribe_amm(pool, mint, amm_acc, Some(sol_vault), Some(tok_vault));

        // SOL vault update
        let mut sol_data = vec![0u8; SPL_TOKEN_ACCOUNT_SIZE];
        let sol_amount: u64 = 50_000_000_000;
        sol_data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
            .copy_from_slice(&sol_amount.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &sol_vault, &sol_data));

        // Token vault update
        let mut tok_data = vec![0u8; SPL_TOKEN_ACCOUNT_SIZE];
        let tok_amount: u64 = 200_000_000;
        tok_data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
            .copy_from_slice(&tok_amount.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &tok_vault, &tok_data));

        let snap = store.get(&pool).expect("snapshot should exist");
        assert_eq!(snap.reserve_sol, Some(50_000_000_000));
        assert_eq!(snap.reserve_token, Some(200_000_000));
    }

    #[test]
    fn test_on_raw_account_update_unknown_pubkey() {
        let sub = YellowstoneSubscriber::new(test_config());
        let store = SnapshotStore::new(SsotConfig::default());
        let unknown = Pubkey::new_unique();

        assert!(!sub.on_raw_account_update(&store, &unknown, &[0u8; 100]));
    }

    // ── Subscription filter generation tests ────────────────────────────

    #[test]
    fn test_build_account_filters_empty() {
        let sub = YellowstoneSubscriber::new(test_config());
        let filters = sub.build_account_filters();
        assert!(filters.is_empty());
    }

    #[test]
    fn test_build_account_filters_bonding_only() {
        let sub = YellowstoneSubscriber::new(test_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bc = Pubkey::new_unique();

        sub.subscribe_bonding_curve(pool, mint, bc);

        let filters = sub.build_account_filters();
        assert!(filters.contains_key("ssot_bonding_curves"));
        assert!(!filters.contains_key("ssot_amm_vaults"));

        let bc_filter = &filters["ssot_bonding_curves"];
        assert_eq!(bc_filter.account.len(), 1);
        assert!(bc_filter.account.contains(&bc.to_string()));
        assert!(bc_filter.owner.is_empty());
    }

    #[test]
    fn test_build_account_filters_amm_vaults() {
        let sub = YellowstoneSubscriber::new(test_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let amm = Pubkey::new_unique();
        let sv = Pubkey::new_unique();
        let tv = Pubkey::new_unique();

        sub.subscribe_amm(pool, mint, amm, Some(sv), Some(tv));

        let filters = sub.build_account_filters();
        assert!(filters.contains_key("ssot_amm_vaults"));

        let amm_filter = &filters["ssot_amm_vaults"];
        assert_eq!(amm_filter.account.len(), 2);
        assert!(amm_filter.account.contains(&sv.to_string()));
        assert!(amm_filter.account.contains(&tv.to_string()));
    }

    #[test]
    fn test_build_account_filters_mixed() {
        let sub = YellowstoneSubscriber::new(test_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bc = Pubkey::new_unique();
        let sv = Pubkey::new_unique();
        let tv = Pubkey::new_unique();

        sub.subscribe_bonding_curve(pool, mint, bc);
        sub.subscribe_amm(pool, mint, Pubkey::new_unique(), Some(sv), Some(tv));

        let filters = sub.build_account_filters();
        assert!(filters.contains_key("ssot_bonding_curves"));
        assert!(filters.contains_key("ssot_amm_vaults"));
    }

    // ── Unsubscribe tests ───────────────────────────────────────────────

    #[test]
    fn test_unsubscribe_pool_clears_all() {
        let sub = YellowstoneSubscriber::new(test_config());
        let store = SnapshotStore::new(SsotConfig::default());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bc = Pubkey::new_unique();
        let sv = Pubkey::new_unique();
        let tv = Pubkey::new_unique();

        sub.subscribe_bonding_curve(pool, mint, bc);
        sub.subscribe_amm(pool, mint, Pubkey::new_unique(), Some(sv), Some(tv));
        assert_eq!(sub.subscribed_accounts().len(), 3); // bc + sv + tv

        // Insert a snapshot so we can verify it's removed on unsubscribe
        store.update_bonding(
            pool,
            mint,
            30_000_000_000,
            1_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
        );
        assert!(store.get(&pool).is_some());

        sub.unsubscribe_pool(&pool, Some(&store));

        assert!(sub.subscribed_accounts().is_empty());
        assert_eq!(sub.subscription_count(), 0);
        // After unsub, vault cache should also be empty
        assert!(sub.vault_cache.read().is_empty());
        // SSOT invariant: snapshot must be removed from store
        assert!(
            store.get(&pool).is_none(),
            "SnapshotStore must be cleared on unsubscribe"
        );
    }

    // ── Unsupported layout test ─────────────────────────────────────────

    #[test]
    fn test_on_raw_account_update_unsupported_token_layout() {
        let sub = YellowstoneSubscriber::new(test_config());
        let store = SnapshotStore::new(SsotConfig::default());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let sol_vault = Pubkey::new_unique();
        let tok_vault = Pubkey::new_unique();

        sub.subscribe_amm(
            pool,
            mint,
            Pubkey::new_unique(),
            Some(sol_vault),
            Some(tok_vault),
        );

        // Token-2022 with extensions: > 165 bytes → UnsupportedLayout
        let data = vec![0u8; 200];
        assert!(!sub.on_raw_account_update(&store, &sol_vault, &data));
        assert_eq!(
            sub.diagnostics
                .unsupported_token_layout
                .load(Ordering::Relaxed),
            1
        );
        assert!(store.get(&pool).is_none());
    }

    // ── Diagnostics counters test ───────────────────────────────────────

    #[test]
    fn test_diagnostics_counters() {
        let sub = YellowstoneSubscriber::new(test_config());
        let store = SnapshotStore::new(SsotConfig::default());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bc_account = Pubkey::new_unique();

        sub.subscribe_bonding_curve(pool, mint, bc_account);

        // Valid bonding curve update
        let mut data = vec![0u8; std::mem::size_of::<BondingCurve>()];
        data[0..8].copy_from_slice(&0x1234_u64.to_le_bytes());
        data[8..16].copy_from_slice(&500_000_000_000_u64.to_le_bytes());
        data[16..24].copy_from_slice(&20_000_000_000_u64.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &bc_account, &data));
        assert_eq!(
            sub.diagnostics
                .curve_updates_applied
                .load(Ordering::Relaxed),
            1
        );

        // Bad curve data
        assert!(!sub.on_raw_account_update(&store, &bc_account, &[0u8; 10]));
        assert_eq!(sub.diagnostics.parse_fail_curve.load(Ordering::Relaxed), 1);

        // Unknown pubkey
        let unknown = Pubkey::new_unique();
        assert!(!sub.on_raw_account_update(&store, &unknown, &[0u8; 100]));
        assert_eq!(sub.diagnostics.ignored_updates.load(Ordering::Relaxed), 1);
    }

    // ── Integration: wire-through test ──────────────────────────────────

    #[test]
    fn test_wire_through_curve_updates_to_store_to_quote() {
        use super::super::quote_engine::{QuoteEngine, QuoteSide};

        let config = SsotConfig {
            bonding_fee_bps_default: 100,
            amm_fee_bps_default: 25,
            slippage_bps_default: 100,
            ..SsotConfig::default()
        };
        let sub = YellowstoneSubscriber::new(config.clone());
        let store = SnapshotStore::new(config.clone());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bc_account = Pubkey::new_unique();

        // 1. Subscribe bonding curve
        sub.subscribe_bonding_curve(pool, mint, bc_account);

        // 2. Curve update #1
        let mut data1 = vec![0u8; std::mem::size_of::<BondingCurve>()];
        data1[0..8].copy_from_slice(&0x1234_u64.to_le_bytes());
        data1[8..16].copy_from_slice(&1_073_000_000_000_000_u64.to_le_bytes()); // v_tokens
        data1[16..24].copy_from_slice(&30_000_000_000_u64.to_le_bytes()); // v_sol
        assert!(sub.on_raw_account_update(&store, &bc_account, &data1));

        let snap1 = store.get(&pool).unwrap();
        assert_eq!(snap1.phase, super::super::phase::PoolPhase::BondingCurve);
        let q1 = QuoteEngine::quote(&snap1, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        // 3. Curve update #2 (changed reserves)
        let mut data2 = vec![0u8; std::mem::size_of::<BondingCurve>()];
        data2[0..8].copy_from_slice(&0x1234_u64.to_le_bytes());
        data2[8..16].copy_from_slice(&900_000_000_000_000_u64.to_le_bytes()); // v_tokens changed
        data2[16..24].copy_from_slice(&35_000_000_000_u64.to_le_bytes()); // v_sol changed
        assert!(sub.on_raw_account_update(&store, &bc_account, &data2));

        let snap2 = store.get(&pool).unwrap();
        let q2 = QuoteEngine::quote(&snap2, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        // Price MUST change after reserves change
        assert!(
            (q1.effective_price - q2.effective_price).abs() > 1e-15,
            "bonding curve prices must differ: q1={}, q2={}",
            q1.effective_price,
            q2.effective_price,
        );

        assert_eq!(
            sub.diagnostics
                .curve_updates_applied
                .load(Ordering::Relaxed),
            2
        );
    }

    #[test]
    fn test_wire_through_amm_vault_updates_to_store_to_quote() {
        use super::super::quote_engine::{QuoteEngine, QuoteSide};

        let config = SsotConfig {
            bonding_fee_bps_default: 100,
            amm_fee_bps_default: 25,
            slippage_bps_default: 100,
            ..SsotConfig::default()
        };
        let sub = YellowstoneSubscriber::new(config.clone());
        let store = SnapshotStore::new(config.clone());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let sol_vault = Pubkey::new_unique();
        let tok_vault = Pubkey::new_unique();

        // 1. Subscribe AMM vaults
        sub.subscribe_amm(
            pool,
            mint,
            Pubkey::new_unique(),
            Some(sol_vault),
            Some(tok_vault),
        );

        // 2. SOL vault update
        let mut sol_data = vec![0u8; SPL_TOKEN_ACCOUNT_SIZE];
        sol_data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
            .copy_from_slice(&50_000_000_000_u64.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &sol_vault, &sol_data));
        // Only one side → no snapshot yet (unless fallback)
        // Push second side to complete:

        // 3. Token vault update
        let mut tok_data = vec![0u8; SPL_TOKEN_ACCOUNT_SIZE];
        tok_data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
            .copy_from_slice(&200_000_000_000_u64.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &tok_vault, &tok_data));

        let snap1 = store.get(&pool).unwrap();
        assert_eq!(snap1.phase, super::super::phase::PoolPhase::Amm);
        let q1 = QuoteEngine::quote(&snap1, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        // 4. Updated reserves (both sides again)
        sol_data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
            .copy_from_slice(&55_000_000_000_u64.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &sol_vault, &sol_data));

        tok_data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
            .copy_from_slice(&180_000_000_000_u64.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &tok_vault, &tok_data));

        let snap2 = store.get(&pool).unwrap();
        let q2 = QuoteEngine::quote(&snap2, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        // Price MUST change after reserves change
        assert!(
            (q1.effective_price - q2.effective_price).abs() > 1e-15,
            "AMM prices must differ: q1={}, q2={}",
            q1.effective_price,
            q2.effective_price,
        );

        assert!(
            sub.diagnostics
                .amm_vault_updates_applied
                .load(Ordering::Relaxed)
                >= 2
        );
    }

    #[test]
    fn test_wire_through_curve_then_amm_migration() {
        use super::super::quote_engine::{QuoteEngine, QuoteSide};

        let config = SsotConfig {
            bonding_fee_bps_default: 100,
            amm_fee_bps_default: 25,
            slippage_bps_default: 100,
            ..SsotConfig::default()
        };
        let sub = YellowstoneSubscriber::new(config.clone());
        let store = SnapshotStore::new(config.clone());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bc_account = Pubkey::new_unique();
        let sol_vault = Pubkey::new_unique();
        let tok_vault = Pubkey::new_unique();

        // Phase 1: Bonding curve
        sub.subscribe_bonding_curve(pool, mint, bc_account);
        let mut bc_data = vec![0u8; std::mem::size_of::<BondingCurve>()];
        bc_data[0..8].copy_from_slice(&0xAB_u64.to_le_bytes());
        bc_data[8..16].copy_from_slice(&1_073_000_000_000_000_u64.to_le_bytes());
        bc_data[16..24].copy_from_slice(&30_000_000_000_u64.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &bc_account, &bc_data));
        let snap_bc = store.get(&pool).unwrap();
        assert_eq!(snap_bc.phase, super::super::phase::PoolPhase::BondingCurve);
        let q_bc = QuoteEngine::quote(&snap_bc, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        // Phase 2: Migration → AMM
        sub.subscribe_amm(
            pool,
            mint,
            Pubkey::new_unique(),
            Some(sol_vault),
            Some(tok_vault),
        );
        let mut sol_data = vec![0u8; SPL_TOKEN_ACCOUNT_SIZE];
        sol_data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
            .copy_from_slice(&80_000_000_000_u64.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &sol_vault, &sol_data));

        let mut tok_data = vec![0u8; SPL_TOKEN_ACCOUNT_SIZE];
        tok_data[SPL_TOKEN_AMOUNT_OFFSET..SPL_TOKEN_AMOUNT_OFFSET + 8]
            .copy_from_slice(&200_000_000_000_u64.to_le_bytes());
        assert!(sub.on_raw_account_update(&store, &tok_vault, &tok_data));

        let snap_amm = store.get(&pool).unwrap();
        assert_eq!(snap_amm.phase, super::super::phase::PoolPhase::Amm);
        let q_amm = QuoteEngine::quote(&snap_amm, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        // After migration, quote source switches and prices differ
        assert!(
            (q_bc.effective_price - q_amm.effective_price).abs() > 1e-15,
            "quote must change after migration: bc={}, amm={}",
            q_bc.effective_price,
            q_amm.effective_price,
        );
    }

    // ── GC vault cache test ─────────────────────────────────────────────

    #[test]
    fn test_gc_vault_cache_evicts_expired() {
        let sub = YellowstoneSubscriber::new(test_config());
        let sol_vault = Pubkey::new_unique();
        let tok_vault = Pubkey::new_unique();

        // Insert entries: one fresh, one expired
        {
            let now_ms = YellowstoneSubscriber::now_ms();
            let mut cache = sub.vault_cache.write();
            cache.insert(
                sol_vault,
                VaultCacheEntry {
                    amount: 100,
                    ts_ms: now_ms, // fresh
                },
            );
            cache.insert(
                tok_vault,
                VaultCacheEntry {
                    amount: 200,
                    ts_ms: now_ms.saturating_sub(VAULT_CACHE_TTL_MS + 1_000), // expired
                },
            );
        }
        assert_eq!(sub.vault_cache.read().len(), 2);

        sub.gc_vault_cache();

        let cache = sub.vault_cache.read();
        assert_eq!(cache.len(), 1, "expired entry should be evicted");
        assert!(
            cache.contains_key(&sol_vault),
            "fresh entry should survive GC"
        );
        assert!(
            !cache.contains_key(&tok_vault),
            "expired entry should be gone"
        );
    }

    // ── Performance guard: no iter().find() in hot path ──────────────────

    #[test]
    fn test_no_iter_find_in_hot_path() {
        // This test ensures no O(n) scans via iter().find() appear in
        // hot-path functions. Cold-path functions (build_account_filters,
        // subscribe_*, unsubscribe_*) are allowed to use iter()/find().
        let source = include_str!("yellowstone.rs");
        // Split at #[cfg(test)] to only check non-test code
        let non_test = source.split("#[cfg(test)]").next().unwrap_or(source);

        // Extract hot-path function bodies by name. These are the functions
        // called on every incoming gRPC account update and must be O(1).
        let hot_path_fns = [
            "fn on_raw_account_update(",
            "fn handle_bonding_curve_update(",
            "fn handle_amm_vault_update(",
        ];

        for fn_sig in &hot_path_fns {
            if let Some(start) = non_test.find(fn_sig) {
                // Take a generous slice from the function start (bodies are <100 lines)
                let slice = &non_test[start..std::cmp::min(start + 3000, non_test.len())];
                assert_eq!(
                    slice.matches(".iter().find(").count(),
                    0,
                    "iter().find() must not appear in hot-path function: {}",
                    fn_sig.trim(),
                );
            }
        }
    }
}
