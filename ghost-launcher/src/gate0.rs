//! Observe-only Gate 0: existing session producers, five frozen snapshots,
//! C/D/E eviction and one JSONL stream. No Oracle/Trigger/execution owner.
mod metrics;
#[cfg(test)]
mod tests;

use crate::{
    session::{OpenSessionRequest, SessionConfig, SessionManager},
    tx_intelligence::{CrossPoolVelocityConfig, FundingSourceConfig},
};
use anyhow::{bail, ensure, Result};
use ghost_brain::{config::GatekeeperV2Config, fast_pipeline::EnhancedCandidate};
use seer::{early_fingerprint::EarlyFingerprintConfig, ipc::DetectedPoolEvent, types::TradeEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Write,
    sync::Arc,
};

pub const PHASES_MS: [u64; 5] = [30_000, 90_000, 180_000, 300_000, 600_000];
pub const OBSERVATION_MS: u64 = 600_000;
// Stały próg definicji Gem, zmieniony na polecenie operatora 2026-09-30.
pub const GEM_MIN_MARKET_CAP_SOL: u64 = 320;
pub const PUMP: Pubkey = solana_sdk::pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const AMM: Pubkey = solana_sdk::pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
pub const WSOL: Pubkey = solana_sdk::pubkey!("So11111111111111111111111111111111111111112");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gate0Config {
    pub source_commitment: seer::config::CommitmentLevel,
    pub admission_ms: u64,
    pub max_active_tokens: usize,
    pub max_events_per_token: usize,
    pub max_events_total: usize,
    pub max_tokens_per_run: usize,
}
impl Default for Gate0Config {
    fn default() -> Self {
        Self {
            // Snapshotów nie cofamy po odrzuceniu spekulacyjnego forka.
            source_commitment: seer::config::CommitmentLevel::Confirmed,
            admission_ms: 36_000_000,
            max_active_tokens: 512,
            max_events_per_token: 16_384,
            max_events_total: 131_072,
            max_tokens_per_run: 100_000,
        }
    }
}
impl Gate0Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=86_400_000).contains(&self.admission_ms),
            "admission_ms outside 1ms..24h"
        );
        ensure!(
            self.max_active_tokens > 0 && self.max_events_per_token >= 101,
            "invalid token/event capacity"
        );
        ensure!(
            self.max_events_total >= self.max_events_per_token
                && self.max_tokens_per_run >= self.max_active_tokens,
            "invalid global capacities"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize)]
struct TransactionOutcome {
    success: bool,
    slot: Option<u64>,
    tx_index: Option<u32>,
    received_ms: u64,
    error_code: Option<String>,
}
impl TransactionOutcome {
    fn from_trade(trade: &TradeEvent, received_ms: u64) -> Self {
        Self {
            success: trade.success,
            slot: trade.slot,
            tx_index: trade.tx_index,
            received_ms,
            error_code: trade.error_code.clone(),
        }
    }
}
#[derive(Debug)]
struct Token {
    mint: Pubkey,
    pool: Pubkey,
    born_ms: u64,
    supply: Option<u64>,
    next_phase: usize,
    next_rate_ms: u64,
    next_anchor: usize,
    swaps: u64,
    successful_tx_signatures: BTreeMap<solana_sdk::signature::Signature, TransactionOutcome>,
    failed_tx_signatures: BTreeMap<solana_sdk::signature::Signature, TransactionOutcome>,
    price_pairs: u64,
    sell_price_pairs: u64,
    max_price_impact_pct: Option<f64>,
    max_sell_impact_pct: Option<f64>,
    retained_events: usize,
    recent_swaps: VecDeque<u64>,
    migration_ms: Option<u64>,
    migration_signature: Option<String>,
    migration_initial_seen: bool,
    post_migration_min_mc_sol: Option<f64>,
    post_migration_floor_breached: bool,
    last_market_cap_sol: Option<f64>,
    last_price_sol: Option<f64>,
    first_price_sol: Option<f64>,
    price_unavailable_reason: Option<&'static str>,
    label_reasons: BTreeSet<String>,
    jito_1s: Option<f64>,
    jito_31s: Option<f64>,
    jito_30s: Option<f64>,
    jito_300s: Option<f64>,
}
#[derive(Debug, Default, Serialize)]
pub struct RunSummary {
    pub admitted: u64,
    pub phase_counts: [u64; 5],
    pub terminal_counts: BTreeMap<String, u64>,
    pub gems: u64,
    pub non_gems: u64,
    pub label_unavailable: u64,
    pub migrations: u64,
    pub migration_initial_states: u64,
    pub completed_with_migration_initial_state: u64,
    pub pool_events_seen: u64,
    pub pool_rejections: BTreeMap<String, u64>,
}
pub struct Gate0<W: Write> {
    config: Gate0Config,
    run_id: String,
    start_ms: u64,
    now_ms: u64,
    sessions: SessionManager,
    gatekeeper: GatekeeperV2Config,
    cpv_config: CrossPoolVelocityConfig,
    funding_config: FundingSourceConfig,
    tokens: BTreeMap<Pubkey, Token>,
    births: BTreeSet<Pubkey>,
    writer: W,
    pub summary: RunSummary,
}
impl<W: Write> Gate0<W> {
    pub fn new(config: Gate0Config, run_id: String, start_ms: u64, writer: W) -> Result<Self> {
        config.validate()?;
        let gatekeeper = GatekeeperV2Config {
            max_wait_time_ms: OBSERVATION_MS,
            min_sol_threshold: 0.0,
            decision_time_series_tx_capacity: config.max_events_per_token,
            enable_alpha_gate: true,
            // Obserwator 600s: historia aktywnych buyerów nie może kończyć się po 16 BUY.
            cpv_per_signer_cap: config.max_events_per_token,
            cpv_global_signer_cap: 100_000,
            ..GatekeeperV2Config::default()
        };
        let sessions = SessionManager::new(SessionConfig {
            default_observation_duration_ms: OBSERVATION_MS,
            max_sessions: config.max_active_tokens,
            ..SessionConfig::default()
        });
        sessions
            .funding_source_index()
            .set_observation_history_ms(OBSERVATION_MS);
        let mut this = Self {
            cpv_config: CrossPoolVelocityConfig::from_gatekeeper_config(&gatekeeper),
            funding_config: FundingSourceConfig::from_gatekeeper_config(&gatekeeper),
            config,
            run_id,
            start_ms,
            now_ms: start_ms,
            sessions,
            gatekeeper,
            tokens: BTreeMap::new(),
            births: BTreeSet::new(),
            writer,
            summary: RunSummary::default(),
        };
        this.emit(
            json!({"kind":"run_start", "schema_version":1, "metric_schema_version":2, "run_id":this.run_id,
            "config":this.config, "phase_ages_ms":PHASES_MS, "observation_ms":OBSERVATION_MS,
            "gem_min_market_cap_sol":GEM_MIN_MARKET_CAP_SOL,
            "e_window_ms":30_000, "e_min_swaps":90, "clock":"consumer_epoch_nondecreasing_wall_and_monotonic",
            "gate_population":{"C_D":"unique_successful_transactions","E":"unique_successful_swaps"}, "source_failed_transactions":true, "execution_enabled":false}),
        )?;
        Ok(this)
    }
    fn emit(&mut self, row: Value) -> Result<()> {
        serde_json::to_writer(&mut self.writer, &row)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }
    fn reject_pool(&mut self, reason: &str) {
        *self
            .summary
            .pool_rejections
            .entry(reason.to_string())
            .or_default() += 1;
    }
    pub fn on_pool(&mut self, event: &DetectedPoolEvent, now: u64) -> Result<()> {
        self.advance(now, false)?;
        self.summary.pool_events_seen = self.summary.pool_events_seen.saturating_add(1);
        let c = &event.candidate;
        if event.observation.as_ref().and_then(|o| o.claims.success) != Some(true) {
            self.reject_pool("observation_success_not_true");
            return Ok(());
        }
        if c.provider_role != Some(ghost_core::RawProviderRoleV1::PrimaryAuthority) {
            self.reject_pool("not_primary_authority");
            return Ok(());
        }
        if c.amm_program_id == AMM {
            if c.quote_mint == WSOL && c.pool_amm_id == canonical_amm(c.base_mint) {
                let mut newly_migrated = false;
                if let Some(token) = self.tokens.get_mut(&c.base_mint) {
                    if token.migration_ms.is_none() {
                        token.migration_ms = Some(now);
                        token.migration_signature = Some(c.signature.clone());
                        newly_migrated = true;
                    }
                }
                if newly_migrated {
                    self.summary.migrations = self.summary.migrations.saturating_add(1);
                }
            }
            return Ok(());
        }
        if c.amm_program_id != PUMP {
            self.reject_pool("not_pump_program");
            return Ok(());
        }
        let native_sol_quote = c.quote_mint == WSOL
            || (c.quote_mint == Pubkey::default()
                && c.creation_regime.quote_regime == ghost_core::PumpQuoteRegimeV1::NativeSol);
        if !native_sol_quote {
            self.reject_pool(if c.quote_mint == Pubkey::default() {
                "default_quote_without_native_sol_evidence"
            } else {
                "non_wsol_quote"
            });
            return Ok(());
        }
        if c.semantic.is_synthetic() {
            self.reject_pool("synthetic");
            return Ok(());
        }
        if now >= self.start_ms.saturating_add(self.config.admission_ms) {
            self.reject_pool("admission_closed");
            return Ok(());
        }
        if self.births.contains(&c.base_mint) {
            self.reject_pool("duplicate_birth");
            return Ok(());
        }
        ensure!(
            self.tokens.len() < self.config.max_active_tokens
                && self.births.len() < self.config.max_tokens_per_run,
            "Gate 0 admission capacity exhausted"
        );
        let candidate = EnhancedCandidate {
            pool_amm_id: c.pool_amm_id,
            base_mint: c.base_mint,
            bonding_curve: c.bonding_curve,
            timestamp: now,
            slot: c.slot,
            signature: c.signature.clone(),
            amm_program_id: c.amm_program_id,
            quote_mint: c.quote_mint,
            token_total_supply: c.token_total_supply,
            initial_liquidity_sol: c.initial_liquidity_sol.unwrap_or(0.0),
            bonding_curve_progress: c.bonding_curve_progress,
            ..EnhancedCandidate::default()
        };
        self.sessions
            .open_session_with_retention(
                OpenSessionRequest {
                    pool_amm_id: c.pool_amm_id,
                    base_mint: c.base_mint,
                    bonding_curve: c.bonding_curve,
                    dev_wallet: (c.creator != Pubkey::default()).then_some(c.creator),
                    candidate_snapshot: candidate,
                    created_at_wall_ms: now,
                    deadline_wall_ms: Some(now + OBSERVATION_MS),
                    gatekeeper_config: self.gatekeeper.clone(),
                    funding_source_config: self.funding_config.clone(),
                    fingerprint_config: EarlyFingerprintConfig {
                        window_secs: 600,
                        ..EarlyFingerprintConfig::default()
                    },
                },
                Some(self.config.max_events_per_token),
            )
            .map_err(|e| anyhow::anyhow!("session open: {e:?}"))?;
        self.sessions
            .get_session(&c.pool_amm_id)
            .ok_or_else(|| anyhow::anyhow!("missing newly opened Gate0 session"))?
            .write()
            .enable_gate0_market_prices();
        self.sessions
            .account_state_core()
            .register_pool_from_bootstrap(
                c.pool_amm_id,
                c.base_mint,
                c.bonding_curve,
                ghost_core::account_state_core::types::BootstrapHints {
                    token_total_supply: c.token_total_supply,
                    ..Default::default()
                },
            );
        self.births.insert(c.base_mint);
        self.summary.admitted += 1;
        self.emit(json!({"kind":"birth","run_id":self.run_id,"mint":c.base_mint.to_string(),"pool":c.pool_amm_id.to_string(),
            "born_ms":now,"create_signature":c.signature,"supply_raw":c.token_total_supply,
            "creator":(c.creator != Pubkey::default()).then_some(c.creator.to_string()),
            "creator_definition":"pump_create_protocol_creator_or_legacy_user"}))?;
        self.tokens.insert(
            c.base_mint,
            Token {
                mint: c.base_mint,
                pool: c.pool_amm_id,
                born_ms: now,
                supply: c.token_total_supply,
                next_phase: 0,
                next_rate_ms: now + 180_000,
                next_anchor: 0,
                swaps: 0,
                successful_tx_signatures: BTreeMap::new(),
                failed_tx_signatures: BTreeMap::new(),
                price_pairs: 0,
                sell_price_pairs: 0,
                max_price_impact_pct: None,
                max_sell_impact_pct: None,
                retained_events: 0,
                recent_swaps: VecDeque::new(),
                migration_ms: None,
                migration_signature: None,
                migration_initial_seen: false,
                post_migration_min_mc_sol: None,
                post_migration_floor_breached: false,
                last_market_cap_sol: None,
                last_price_sol: None,
                first_price_sol: None,
                price_unavailable_reason: Some("no_verified_reserve_observation"),
                label_reasons: BTreeSet::new(),
                jito_1s: None,
                jito_31s: None,
                jito_30s: None,
                jito_300s: None,
            },
        );
        Ok(())
    }
    pub fn on_trade(&mut self, trade: &TradeEvent, now: u64) -> Result<()> {
        self.advance(now, false)?;
        if trade.provider_role != Some(ghost_core::RawProviderRoleV1::PrimaryAuthority)
            || trade.semantic.is_synthetic()
        {
            return Ok(());
        }
        let mut tx = crate::components::seer::trade_event_to_pool_transaction(trade);
        let physical_pool = trade.pool_amm_id;
        // Jeden rynek logiczny curve→canonical AMM; inne pule tego samego minta
        // pozostają odrębnymi rynkami w globalnym CPV.
        if trade.is_pumpswap && physical_pool == canonical_amm(trade.mint) {
            tx.pool_amm_id = self
                .tokens
                .get(&trade.mint)
                .map(|token| token.pool)
                .unwrap_or_else(|| canonical_curve(trade.mint))
                .to_string();
        }
        // Identyczna normalizacja również przed obsługą redostawy. Nie używamy
        // ostatniej ceny tokena jako rzekomej ceny tego zdarzenia.
        tx.price_quote = observed_trade_price(trade);
        if trade.is_pumpswap {
            tx.v_sol_in_bonding_curve = None;
            tx.v_tokens_in_bonding_curve = None;
            tx.virtual_sol_reserves = None;
            tx.virtual_token_reserves = None;
            tx.curve_data_known = false;
        }
        let initialization = trade.is_pumpswap && trade.is_dev_buy;
        if !initialization {
            // Sprawdzamy sprzeczność przed mutacją wspólnego indeksu CPV.
            self.check_transaction_outcome(trade, now)?;
            self.sessions
                .cross_pool_velocity_index()
                .observe_transaction_at(&tx.pool_amm_id, &tx, now, &self.cpv_config);
        }
        let Some(token) = self.tokens.get_mut(&trade.mint) else {
            return Ok(());
        };
        let expected_pool = if trade.is_pumpswap {
            canonical_amm(token.mint)
        } else {
            token.pool
        };
        if physical_pool != expected_pool {
            return Ok(());
        }

        // PumpSwap CreatePool compatibility BUY is migration-state evidence,
        // not a trade sample. Consume it before session keying/retention so it
        // cannot inflate C/D/E, while still requiring authoritative success
        // and the canonical PumpSwap pool.
        if initialization {
            if !trade.metadata_availability.status_known {
                token
                    .label_reasons
                    .insert("unknown_transaction_status".into());
                return Ok(());
            }
            if !trade.success {
                return Ok(());
            }
            if let Some(state) = trade
                .amm_observation
                .as_ref()
                .filter(|state| state.pool == expected_pool && state.initialization)
            {
                // Producent initialization=true wymaga surowego CreatePoolEvent
                // z dopasowanym programem, mintem, pulą i WSOL. To samodzielny
                // dowód utworzenia kanonicznej AMM, nawet gdy osobny PoolDetected
                // jeszcze nie dotarł lub bundle zawiera kilka inicjalizacji.
                if token.migration_ms.is_none() {
                    token.migration_ms = Some(now);
                    token.migration_signature = Some(trade.signature.to_string());
                    self.summary.migrations = self.summary.migrations.saturating_add(1);
                }
                let initial_seen_before = token.migration_initial_seen;
                token.observe_amm(state, &trade.signature.to_string());
                if !initial_seen_before && token.migration_initial_seen {
                    self.summary.migration_initial_states =
                        self.summary.migration_initial_states.saturating_add(1);
                }
            } else {
                token.label_reasons.insert("missing_amm_state".into());
            }
            return Ok(());
        }

        // Failed attempts are observations, not executed flow. The historical
        // producer assumes successful input and would otherwise add failed
        // BUY amounts to volume/holders. Keep the separate transaction counter.
        if trade.metadata_availability.status_known && !trade.success {
            if !token.failed_tx_signatures.contains_key(&trade.signature) {
                ensure!(
                    token.retained_events < self.config.max_events_per_token,
                    "Gate 0 per-token retention capacity exhausted"
                );
                token
                    .failed_tx_signatures
                    .entry(trade.signature)
                    .or_insert_with(|| TransactionOutcome::from_trade(trade, now));
                token.retained_events += 1;
            }
            ensure!(
                self.tokens
                    .values()
                    .map(|t| t.retained_events)
                    .sum::<usize>()
                    <= self.config.max_events_total,
                "Gate 0 global retention capacity exhausted"
            );
            return Ok(());
        }
        let session = self
            .sessions
            .get_session(&token.pool)
            .ok_or_else(|| anyhow::anyhow!("missing observation session"))?;
        let key = crate::session::observation::SessionTransactionKey::for_transaction(&tx)
            .ok_or_else(|| anyhow::anyhow!("unkeyable Gate 0 trade"))?;
        if session.read().tx_keys_seen.contains(&key) {
            session.write().ingest_transaction(Arc::new(tx));
            return Ok(());
        }
        let before = session.read().tx_keys_seen.len();
        ensure!(
            token.retained_events < self.config.max_events_per_token,
            "Gate 0 per-token retention capacity exhausted"
        );
        if trade.metadata_availability.status_known && !trade.success { /* Keep failed attempts in the existing metric producer. */
        } else if !trade.metadata_availability.status_known {
            token
                .label_reasons
                .insert("unknown_transaction_status".into());
        } else if trade.is_pumpswap {
            if let Some(state) = trade
                .amm_observation
                .as_ref()
                .filter(|s| s.pool == canonical_amm(token.mint))
            {
                let initial_seen_before = token.migration_initial_seen;
                token.observe_amm(state, &trade.signature.to_string());
                if !initial_seen_before && token.migration_initial_seen {
                    self.summary.migration_initial_states =
                        self.summary.migration_initial_states.saturating_add(1);
                }
                tx.market_cap_sol = token.last_market_cap_sol;
            } else {
                token.label_reasons.insert("missing_amm_state".into());
                token.invalidate_price("missing_amm_state");
            }
        } else {
            token.observe_curve(trade);
        }
        session.write().ingest_transaction(Arc::new(tx));
        if session.read().tx_keys_seen.len() > before {
            token.retained_events += 1;
            if trade.metadata_availability.status_known && trade.success {
                token.swaps += 1;
                token
                    .successful_tx_signatures
                    .entry(trade.signature)
                    .or_insert_with(|| TransactionOutcome::from_trade(trade, now));
                token.recent_swaps.push_back(now);
                token.observe_price_impact(trade);
            } else if trade.metadata_availability.status_known {
                token
                    .failed_tx_signatures
                    .entry(trade.signature)
                    .or_insert_with(|| TransactionOutcome::from_trade(trade, now));
            }
        }
        ensure!(
            self.tokens
                .values()
                .map(|t| t.retained_events)
                .sum::<usize>()
                <= self.config.max_events_total,
            "Gate 0 global retention capacity exhausted"
        );
        Ok(())
    }
    fn check_transaction_outcome(&mut self, trade: &TradeEvent, now: u64) -> Result<()> {
        if !trade.metadata_availability.status_known {
            return Ok(());
        }
        let Some(token) = self.tokens.get(&trade.mint) else {
            return Ok(());
        };
        let expected_pool = if trade.is_pumpswap {
            canonical_amm(token.mint)
        } else {
            token.pool
        };
        if trade.pool_amm_id != expected_pool {
            return Ok(());
        }
        let previous = if trade.success {
            token.failed_tx_signatures.get(&trade.signature)
        } else {
            token.successful_tx_signatures.get(&trade.signature)
        }
        .cloned();
        if let Some(previous) = previous {
            self.emit(json!({
                "kind":"transaction_outcome_conflict", "run_id":self.run_id,
                "mint":trade.mint.to_string(), "pool":trade.pool_amm_id.to_string(),
                "signature":trade.signature.to_string(), "provider_id":trade.provider_id,
                "first":previous, "conflicting":TransactionOutcome::from_trade(trade, now)
            }))?;
            bail!("conflicting transaction outcome in Gate0: mint={} signature={} first_slot={:?} conflicting_slot={:?}",
                trade.mint, trade.signature, previous.slot, trade.slot);
        }
        Ok(())
    }
    pub fn on_progress(
        &mut self,
        p: &seer::types::PrimaryTradeFeedProgressV1,
        now: u64,
    ) -> Result<()> {
        if p.gap {
            let reason = p
                .gap_reason
                .map(seer::types::PrimaryTradeFeedGapReasonV1::as_str)
                .unwrap_or("unspecified");
            if self.summary.admitted > 0 {
                return self.fail_source(now, &format!("source_gap:{reason}"));
            }
            self.sessions
                .cross_pool_velocity_index()
                .mark_stream_gap(now);
            return Ok(());
        }
        self.advance(now, false)?;
        self.sessions
            .cross_pool_velocity_index()
            .observe_source_progress(p.epoch, p.event_ms, p.received_ms, now, &self.cpv_config);
        Ok(())
    }
    pub fn on_funding(
        &mut self,
        event: &seer::ipc::DetectedFundingTransferEvent,
        now: u64,
    ) -> Result<()> {
        self.advance(now, false)?;
        let t = &event.transfer;
        let transfer = crate::events::FundingTransferObserved {
            semantic: t.semantic,
            slot: t.slot,
            event_ordinal: t.event_ordinal,
            tx_index: t.tx_index,
            outer_instruction_index: t.outer_instruction_index,
            inner_group_index: t.inner_group_index,
            cpi_stack_height: t.cpi_stack_height,
            event_time: t.event_time,
            arrival_ts_ms: t.arrival_ts_ms,
            signature: t.signature.clone(),
            source_wallet: t.source_wallet.clone(),
            recipient_wallet: t.recipient_wallet.clone(),
            lamports: t.lamports,
            full_chain_coverage: t.full_chain_coverage,
            provenance: t.provenance,
            lane_health: event.lane_health,
            detected_at: event.detected_at,
            sequence_number: event.sequence_number,
        };
        self.sessions
            .funding_source_index()
            .observe_lane_health(event.lane_health);
        self.sessions
            .funding_source_index()
            .observe_transfer(&transfer, &self.funding_config);
        Ok(())
    }
    pub fn set_funding_available(&self, available: bool) {
        self.sessions.set_funding_stream_available(available);
    }
    pub fn on_account(
        &mut self,
        a: &seer::ipc::DetectedAccountUpdateEvent,
        now: u64,
    ) -> Result<()> {
        self.advance(now, false)?;
        // AMM pool bytes are not reserve-vault balances. Do not feed their legacy
        // compatibility decoding into the Gem price path.
        if a.source_account_owner_or_program != Some(PUMP)
            || a.provider_role != Some(ghost_core::RawProviderRoleV1::PrimaryAuthority)
        {
            return Ok(());
        }
        let Some(token) = self.tokens.get(&a.base_mint) else {
            return Ok(());
        };
        if token.migration_ms.is_some() || a.bonding_curve != token.pool {
            return Ok(());
        }
        let update = ghost_core::account_state_core::types::AccountStateUpdate {
            pool_amm_id: token.pool,
            base_mint: a.base_mint,
            bonding_curve: a.bonding_curve,
            sol_reserves: a.sol_reserves,
            token_reserves: a.token_reserves,
            is_complete: a.complete,
            slot: a.slot,
            write_version: a.write_version,
            source_account_pubkey: a.source_account_pubkey,
            source_account_owner_or_program: a.source_account_owner_or_program,
            account_data_len: a.account_data_len,
            account_data_hash: a.account_data_hash.clone(),
            receive_ts_ms: now,
            receive_seq: a.sequence_number,
            curve_finality: a.curve_finality,
            source: ghost_core::account_state_core::types::UpdateSource::GeyserAccountUpdate,
            provider_id: a.provider_id.clone(),
            provider_role: a.provider_role,
            txn_signature: a.txn_signature,
        };
        if let Some(session) = self.sessions.get_session(&token.pool) {
            session.write().on_account_update(&update);
        }
        Ok(())
    }
    pub fn tick(&mut self, now: u64) -> Result<()> {
        self.advance(now, true)
    }
    pub fn drained(&self, now: u64) -> bool {
        now >= self.start_ms.saturating_add(self.config.admission_ms) && self.tokens.is_empty()
    }
    pub fn active_tokens(&self) -> usize {
        self.tokens.len()
    }
    pub fn fail_source(&mut self, now: u64, reason: &str) -> Result<()> {
        self.sessions
            .cross_pool_velocity_index()
            .mark_stream_gap(now);
        self.sessions.set_funding_stream_available(false);
        let mints: Vec<_> = self.tokens.keys().copied().collect();
        for mint in mints {
            self.finish_token(mint, now, reason)?;
        }
        bail!("Gate 0 stopped: {reason}")
    }
    fn advance(&mut self, now: u64, inclusive: bool) -> Result<()> {
        ensure!(now >= self.now_ms, "non-monotonic Gate 0 clock");
        self.now_ms = now;
        loop {
            let due = self
                .tokens
                .iter()
                .filter_map(|(mint, t)| {
                    let phase = PHASES_MS.get(t.next_phase).map(|age| t.born_ms + age);
                    let anchor = [1_000, 31_000]
                        .get(t.next_anchor)
                        .map(|age| t.born_ms + age);
                    let next = phase
                        .into_iter()
                        .chain(anchor)
                        .chain(Some(t.next_rate_ms))
                        .min()?;
                    ((next < now) || (inclusive && next == now)).then_some((*mint, next))
                })
                .min_by_key(|(mint, time)| (*time, *mint));
            let Some((mint, at)) = due else {
                break;
            };
            let t = self.tokens.get(&mint).unwrap();
            let age = at - t.born_ms;
            if [1_000, 31_000].get(t.next_anchor) == Some(&age) {
                let value = self
                    .sessions
                    .get_session(&t.pool)
                    .and_then(|s| s.read().fingerprint_metrics())
                    .and_then(|f| f.jito_tip_intensity);
                let t = self.tokens.get_mut(&mint).unwrap();
                if t.next_anchor == 0 {
                    t.jito_1s = value;
                } else {
                    t.jito_31s = value;
                }
                t.next_anchor += 1;
            }
            let t = self.tokens.get(&mint).unwrap();
            if PHASES_MS.get(t.next_phase) == Some(&age) {
                self.emit_phase(mint, at)?;
            }
            let t = self.tokens.get_mut(&mint).unwrap();
            while t
                .recent_swaps
                .front()
                .is_some_and(|x| *x <= at.saturating_sub(30_000))
            {
                t.recent_swaps.pop_front();
            }
            let reason = if age == 30_000 && t.successful_tx_signatures.len() < 20 {
                Some("C")
            } else if age == 180_000 && t.successful_tx_signatures.len() <= 100 {
                Some("D")
            } else if at == t.next_rate_ms && t.recent_swaps.len() < 90 {
                Some("E")
            } else if age == OBSERVATION_MS {
                Some("completed")
            } else {
                None
            };
            if at == t.next_rate_ms {
                t.next_rate_ms += 1_000;
            }
            if let Some(reason) = reason {
                self.finish_token(mint, at, reason)?;
            }
        }
        Ok(())
    }
    fn emit_phase(&mut self, mint: Pubkey, at: u64) -> Result<()> {
        let t = self.tokens.get(&mint).unwrap();
        let session = self
            .sessions
            .get_session(&t.pool)
            .ok_or_else(|| anyhow::anyhow!("missing session at checkpoint"))?;
        let snapshot = metrics::build(&session.read(), t, at, &self.gatekeeper)?;
        let phase = t.next_phase;
        let frozen_jito = snapshot["metrics"]["jito_tip_intensity"].as_f64();
        self.emit(json!({"kind":"phase", "schema_version":1, "run_id":self.run_id, "mint":mint.to_string(),
            "phase":phase+1, "age_ms":PHASES_MS[phase], "cutoff_ms":at, "snapshot":snapshot}))?;
        let t = self.tokens.get_mut(&mint).unwrap();
        if phase == 0 {
            t.jito_30s = frozen_jito;
        }
        if phase == 3 {
            t.jito_300s = frozen_jito;
        }
        t.next_phase += 1;
        self.summary.phase_counts[phase] += 1;
        Ok(())
    }
    fn finish_token(&mut self, mint: Pubkey, at: u64, reason: &str) -> Result<()> {
        let t = self.tokens.get(&mint).unwrap();
        let gem = t.label(reason);
        let pool = t.pool;
        let completed_with_migration_initial_state =
            reason == "completed" && t.migration_initial_seen;
        self.emit(json!({"kind":"terminal", "run_id":self.run_id, "mint":mint.to_string(),
            "age_ms":at.saturating_sub(t.born_ms), "last_phase":t.next_phase, "reason":reason, "gem":gem,
            "successful_swap_count":t.swaps,"unique_successful_tx_count":t.successful_tx_signatures.len(),"unique_failed_tx_count":t.failed_tx_signatures.len(), "migration_age_ms":t.migration_ms.map(|v|v.saturating_sub(t.born_ms)),
            "post_migration_min_mc_sol":t.post_migration_min_mc_sol, "migration_initial_seen":t.migration_initial_seen,
            "label_reasons":t.label_reasons, "label_horizon_ms":OBSERVATION_MS}))?;
        self.tokens.remove(&mint);
        self.sessions.remove_session(&pool);
        self.sessions.account_state_core().remove_pool(&mint);
        *self
            .summary
            .terminal_counts
            .entry(reason.to_string())
            .or_default() += 1;
        if completed_with_migration_initial_state {
            self.summary.completed_with_migration_initial_state = self
                .summary
                .completed_with_migration_initial_state
                .saturating_add(1);
        }
        match gem {
            Some(true) => self.summary.gems += 1,
            Some(false) => self.summary.non_gems += 1,
            None => self.summary.label_unavailable += 1,
        }
        Ok(())
    }
    pub fn close(
        mut self,
        now: u64,
        reason: Option<&str>,
        shutdown_error: Option<&str>,
        runtime_diagnostics: Value,
    ) -> Result<W> {
        let mints: Vec<_> = self.tokens.keys().copied().collect();
        for mint in mints {
            self.finish_token(mint, now, reason.unwrap_or("incomplete_run"))?;
        }
        self.emit(json!({
            "kind":"run_end",
            "run_id":self.run_id,
            "reason":reason,
            "shutdown_error":shutdown_error,
            "runtime_diagnostics":runtime_diagnostics,
            "summary":self.summary
        }))?;
        Ok(self.writer)
    }
}
impl Token {
    fn invalidate_price(&mut self, reason: &'static str) {
        self.last_market_cap_sol = None;
        self.last_price_sol = None;
        self.price_unavailable_reason = Some(reason);
    }
    fn observe_price_impact(&mut self, trade: &TradeEvent) {
        let Some((before, after)) = observed_trade_price_pair(trade) else {
            return;
        };
        let impact = (after / before - 1.0) * 100.0;
        if !impact.is_finite() {
            return;
        }
        self.price_pairs += 1;
        self.max_price_impact_pct =
            Some(self.max_price_impact_pct.unwrap_or(0.0).max(impact.abs()));
        if !trade.is_buy {
            self.sell_price_pairs += 1;
            self.max_sell_impact_pct = Some(
                self.max_sell_impact_pct
                    .unwrap_or(0.0)
                    .max((-impact).max(0.0)),
            );
        }
    }
    fn label(&self, reason: &str) -> Option<bool> {
        if !matches!(reason, "C" | "D" | "E" | "completed") {
            return None;
        }
        if self.label_reasons.contains("unknown_transaction_status") {
            return None;
        }
        if reason != "completed" {
            return Some(false);
        }
        let Some(migration) = self.migration_ms else {
            return Some(false);
        };
        if migration.saturating_sub(self.born_ms) <= 3_000 || self.post_migration_floor_breached {
            return Some(false);
        }
        if !self.migration_initial_seen
            || self.post_migration_min_mc_sol.is_none()
            || !self.label_reasons.is_empty()
        {
            return None;
        }
        Some(true)
    }
    fn record_reserves(&mut self, base: u64, quote: u64, supply: u64, migrated: bool) {
        if base == 0 || supply == 0 {
            self.label_reasons
                .insert("invalid_reserves_or_supply".into());
            self.invalidate_price("invalid_reserves_or_supply");
            return;
        }
        let numerator = u128::from(quote) * u128::from(supply);
        let denominator = u128::from(base) * 1_000_000_000;
        let mc = numerator as f64 / denominator as f64;
        self.last_market_cap_sol = Some(mc);
        let price = quote as f64 / base as f64 / 1_000.0;
        self.last_price_sol = Some(price);
        self.first_price_sol.get_or_insert(price);
        self.price_unavailable_reason = None;
        if migrated {
            self.post_migration_min_mc_sol =
                Some(self.post_migration_min_mc_sol.map_or(mc, |old| old.min(mc)));
            self.post_migration_floor_breached |=
                numerator < u128::from(GEM_MIN_MARKET_CAP_SOL) * denominator;
        }
    }
    fn observe_curve(&mut self, trade: &TradeEvent) {
        if let (Some(base), Some(quote), Some(supply)) = (
            trade.virtual_token_reserves,
            trade.virtual_sol_reserves,
            self.supply,
        ) {
            if self.migration_ms.is_none() {
                self.record_reserves(base, quote, supply, false);
            }
        }
    }
    fn observe_amm(&mut self, s: &seer::amm_observation::AmmObservation, signature: &str) {
        if self.migration_ms.is_none() {
            self.label_reasons.insert("missing_migration_create".into());
            self.invalidate_price("missing_migration_create");
            return;
        }
        let supply = s
            .base_supply
            .or_else(|| s.initialization.then_some(self.supply).flatten());
        let Some(supply) = supply else {
            self.label_reasons.insert("missing_amm_supply".into());
            self.invalidate_price("missing_amm_supply");
            return;
        };
        let quote = |raw: u64| {
            i128::from(raw)
                .checked_add(s.virtual_quote)
                .and_then(|v| u64::try_from(v).ok())
        };
        if let (Some(base), Some(raw_quote)) = (s.pre_base, s.pre_quote) {
            if let Some(q) = quote(raw_quote) {
                self.record_reserves(base, q, supply, true);
            } else {
                self.label_reasons
                    .insert("invalid_effective_quote_reserves".into());
            }
        }
        if let Some(q) = quote(s.quote_reserves) {
            self.record_reserves(s.base_reserves, q, supply, true);
        } else {
            self.label_reasons
                .insert("invalid_effective_quote_reserves".into());
            self.invalidate_price("invalid_effective_quote_reserves");
        }
        if self.migration_signature.as_deref() == Some(signature) {
            self.migration_initial_seen = true;
        }
        self.supply = Some(supply);
    }
}

fn reserve_price(base: u64, quote: u64) -> Option<f64> {
    if base == 0 || quote == 0 {
        return None;
    }
    let price = quote as f64 / base as f64 / 1_000.0;
    (price.is_finite() && price > 0.0).then_some(price)
}
fn effective_amm_quote(raw: u64, virtual_quote: i128) -> Option<u64> {
    u64::try_from(i128::from(raw).checked_add(virtual_quote)?).ok()
}
fn observed_trade_price(trade: &TradeEvent) -> Option<f64> {
    if !trade.metadata_availability.status_known || !trade.success {
        return None;
    }
    if trade.is_pumpswap {
        let s = trade
            .amm_observation
            .as_ref()
            .filter(|s| s.pool == trade.pool_amm_id)?;
        reserve_price(
            s.base_reserves,
            effective_amm_quote(s.quote_reserves, s.virtual_quote)?,
        )
    } else {
        reserve_price(trade.virtual_token_reserves?, trade.virtual_sol_reserves?)
    }
}
fn observed_trade_price_pair(trade: &TradeEvent) -> Option<(f64, f64)> {
    let after = observed_trade_price(trade)?;
    let before = if trade.is_pumpswap {
        let s = trade.amm_observation.as_ref()?;
        reserve_price(
            s.pre_base?,
            effective_amm_quote(s.pre_quote?, s.virtual_quote)?,
        )?
    } else {
        // Raw post reserves istnieją tylko dla zdekodowanego TradeEvent;
        // kwoty fallback/slippage bez takiego eventu nie dostają pary ceny.
        let (base, quote) = (trade.virtual_token_reserves?, trade.virtual_sol_reserves?);
        if trade.is_buy {
            reserve_price(
                base.checked_add(trade.amount)?,
                quote.checked_sub(trade.max_sol_cost)?,
            )?
        } else {
            reserve_price(
                base.checked_sub(trade.amount)?,
                quote.checked_add(trade.min_sol_output)?,
            )?
        }
    };
    Some((before, after))
}
fn canonical_curve(mint: Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &PUMP).0
}

/// The canonical SOL pool created by Pump's migrate instruction, not an
/// arbitrary externally created market using the same mint.
pub fn canonical_amm(mint: Pubkey) -> Pubkey {
    let authority = Pubkey::find_program_address(&[b"pool-authority", mint.as_ref()], &PUMP).0;
    Pubkey::find_program_address(
        &[
            b"pool",
            &0u16.to_le_bytes(),
            authority.as_ref(),
            mint.as_ref(),
            WSOL.as_ref(),
        ],
        &AMM,
    )
    .0
}
