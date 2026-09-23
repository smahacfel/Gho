//! Punktowe potwierdzenie aktualności bajtów dla istniejącego wyjścia shadow.
//! Odczyt nie przechodzi przez reducer i nie zmienia zegara aktywności poola.

use super::*;
use solana_account_decoder::UiAccountEncoding;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcAccountInfoConfig};
use solana_sdk::{account::Account, commitment_config::CommitmentConfig};

const PUMP_PROGRAM: Pubkey = solana_sdk::pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
// Anchor: pierwsze 8 bajtów SHA256("account:BondingCurve").
const CURVE_DISCRIMINATOR: [u8; 8] = [23, 183, 248, 55, 96, 216, 172, 96];

#[derive(Debug, Clone, Serialize)]
pub(super) struct QuoteFreshnessConfirmation {
    pub(super) action_id: String,
    position_id: String,
    position_epoch: u64,
    requested_state_revision: u64,
    remaining_quantity_raw: u64,
    account: Pubkey,
    owner: Pubkey,
    account_data_hash: String,
    canonical_slot: u64,
    canonical_timestamp_ms: u64,
    min_context_slot: u64,
    pub(super) rpc_context_slot: u64,
    requested_at_ms: u64,
    received_at_ms: u64,
    recovery_deadline_ms: u64,
    commitment: &'static str,
    #[serde(skip)]
    requested_at: Instant,
    #[serde(skip)]
    canonical: CanonicalPoolState,
}

pub(super) struct PendingQuoteConfirmation {
    action: ShadowExitActionHandle,
    receiver: oneshot::Receiver<Result<QuoteFreshnessConfirmation, &'static str>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for PendingQuoteConfirmation {
    fn drop(&mut self) {
        // Usunięcie pozycji, shutdown lub nieważny guard anuluje również HTTP.
        self.task.abort();
    }
}

pub(super) enum ConfirmationPoll {
    Absent,
    Pending,
    Ready(QuoteFreshnessConfirmation),
}

impl QuoteFreshnessConfirmation {
    fn new(
        action: &ShadowExitActionHandle,
        canonical: CanonicalPoolState,
        now_ms: u64,
    ) -> Result<Self, &'static str> {
        let expected_curve = Pubkey::find_program_address(
            &[b"bonding-curve", action.base_mint.as_ref()],
            &PUMP_PROGRAM,
        )
        .0;
        if canonical.base_mint != action.base_mint
            || canonical.bonding_curve != expected_curve
            || canonical.source_account_pubkey != Some(expected_curve)
            || canonical.source_account_owner_or_program != Some(PUMP_PROGRAM)
        {
            return Err("canonical_identity_mismatch");
        }
        if canonical.state_phase != StatePhase::Canonical || canonical.is_complete {
            return Err("canonical_curve_not_executable");
        }
        if canonical.last_update_slot == 0 || canonical.account_data_len.is_none() {
            return Err("canonical_provenance_missing");
        }
        let hash = canonical
            .account_data_hash
            .clone()
            .ok_or("canonical_hash_missing")?;
        Ok(Self {
            action_id: action.action_id.clone(),
            position_id: action.position_id.clone(),
            position_epoch: action.position_epoch,
            requested_state_revision: action.state_revision,
            remaining_quantity_raw: action.expected_remaining_quantity,
            account: expected_curve,
            owner: PUMP_PROGRAM,
            account_data_hash: hash,
            canonical_slot: canonical.last_update_slot,
            min_context_slot: canonical.last_update_slot,
            canonical_timestamp_ms: canonical
                .last_observed_ts_ms
                .max(canonical.last_update_ts_ms),
            rpc_context_slot: 0,
            requested_at_ms: now_ms,
            received_at_ms: 0,
            recovery_deadline_ms: action.recovery_deadline_ms,
            commitment: "processed",
            requested_at: Instant::now(),
            canonical,
        })
    }

    fn validate_account(&self, account: &Account, context_slot: u64) -> Result<(), &'static str> {
        if account.owner != self.owner || account.executable {
            return Err("rpc_owner_mismatch");
        }
        if account.data.get(..8) != Some(CURVE_DISCRIMINATOR.as_slice()) {
            return Err("rpc_discriminator_mismatch");
        }
        if context_slot == 0 || context_slot < self.min_context_slot {
            return Err("rpc_context_slot_too_old");
        }
        if Some(account.data.len() as u64) != self.canonical.account_data_len
            || blake3::hash(&account.data).to_hex().as_str() != self.account_data_hash
        {
            return Err("rpc_bytes_diverge");
        }
        let curve = seer::curve_parser::parse_curve_from_account(&account.data)
            .map_err(|_| "rpc_curve_parse_failed")?;
        if curve.complete != 0
            || curve.virtual_sol_reserves != self.canonical.virtual_sol_reserves
            || curve.virtual_token_reserves != self.canonical.virtual_token_reserves
        {
            return Err("rpc_reserves_mismatch");
        }
        Ok(())
    }

    fn is_fresh(&self, now_ms: u64, ttl_ms: u64) -> bool {
        // Wiek liczony od początku zapytania, także zegarem monotonicznym.
        self.rpc_context_slot >= self.min_context_slot
            && now_ms >= self.requested_at_ms
            && now_ms < self.recovery_deadline_ms
            && now_ms.saturating_sub(self.requested_at_ms) <= ttl_ms
            && self.requested_at.elapsed() <= Duration::from_millis(ttl_ms)
            && self.requested_at.elapsed()
                < Duration::from_millis(
                    self.recovery_deadline_ms
                        .saturating_sub(self.requested_at_ms),
                )
    }
}

impl MonitoringEngine {
    pub fn set_shadow_quote_confirmation_rpc(&mut self, rpc: Arc<RpcClient>) {
        self.shadow_quote_confirmation_rpc = Some(rpc);
    }

    pub(super) fn start_quote_confirmation(&self, action: &ShadowExitActionHandle, now_ms: u64) {
        let Some(rpc) = self.shadow_quote_confirmation_rpc.as_ref() else {
            return;
        };
        if now_ms >= action.recovery_deadline_ms || action.crash_guard_quote_requirement.is_some() {
            return;
        }
        let Some(canonical) = self.current_canonical_state(&action.base_mint) else {
            return;
        };
        let mut positions = self.positions.write();
        let Some(pos) = positions.get_mut(&action.base_mint) else {
            return;
        };
        if Self::validate_action_handle(pos, action).is_err()
            || pos.pending_quote_confirmation.is_some()
            || pos.lane != Lane::Shadow
            || pos.rug_scalp_facts.is_some()
            || pos.bonding_curve != canonical.bonding_curve
            || pos.pool_amm_id != canonical.pool_amm_id
        {
            return;
        }
        let mut confirmation = match QuoteFreshnessConfirmation::new(action, canonical, now_ms) {
            Ok(confirmation) => confirmation,
            Err(reason) => {
                warn!(action_id = %action.action_id, reason, "shadow_quote_confirmation_rejected");
                return;
            }
        };
        // Globalny slot jest tylko dolną granicą odczytu RPC. Sam w sobie
        // nigdy nie odświeża ceny, aktywności ani canonical state tego poola.
        confirmation.min_context_slot = self
            .account_state_core
            .as_ref()
            .and_then(|core| core.latest_observed_slot())
            .unwrap_or(confirmation.canonical_slot)
            .max(confirmation.canonical_slot);
        let timeout = Duration::from_millis(
            action
                .recovery_deadline_ms
                .saturating_sub(now_ms)
                .min(self.shadow_exit_stale_after_ms()),
        );
        let rpc = Arc::clone(rpc);
        let (sender, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            let result = async {
                let response = tokio::time::timeout(
                    timeout,
                    rpc.get_account_with_config(
                        &confirmation.account,
                        RpcAccountInfoConfig {
                            encoding: Some(UiAccountEncoding::Base64),
                            commitment: Some(CommitmentConfig::processed()),
                            min_context_slot: Some(confirmation.min_context_slot),
                            ..RpcAccountInfoConfig::default()
                        },
                    ),
                )
                .await
                .map_err(|_| "rpc_timeout")?
                .map_err(|_| "rpc_transport_or_response_error")?;
                let account = response.value.ok_or("rpc_account_missing")?;
                confirmation.validate_account(&account, response.context.slot)?;
                confirmation.rpc_context_slot = response.context.slot;
                confirmation.received_at_ms = current_time_ms();
                Ok(confirmation)
            }
            .await;
            let _ = sender.send(result);
        });
        info!(action_id = %action.action_id, base_mint = %action.base_mint,
            state_revision = action.state_revision, "shadow_quote_confirmation_requested");
        pos.pending_quote_confirmation = Some(PendingQuoteConfirmation {
            action: action.clone(),
            receiver,
            task,
        });
    }

    pub(super) fn poll_quote_confirmation(
        &self,
        base_mint: &Pubkey,
        guard: &PositionSnapshotGuard,
        now_ms: u64,
    ) -> ConfirmationPoll {
        let mut positions = self.positions.write();
        let Some(pos) = positions.get_mut(base_mint) else {
            return ConfirmationPoll::Absent;
        };
        let Some(mut pending) = pos.pending_quote_confirmation.take() else {
            return ConfirmationPoll::Absent;
        };
        let result = if Self::validate_action_handle(pos, &pending.action).is_err()
            || Self::validate_snapshot_guard(pos, guard).is_err()
        {
            Err("position_guard_changed")
        } else if now_ms >= pending.action.recovery_deadline_ms {
            Err("recovery_deadline_elapsed")
        } else if pos
            .pending_exit_proposal
            .as_ref()
            .and_then(|proposal| proposal.last_quote_attempt_ms)
            .is_some_and(|last| now_ms.saturating_sub(last) < SHADOW_QUOTE_RETRY_INTERVAL_MS)
        {
            // Tick może nadejść np. po 498 ms. Nie konsumujemy dowodu,
            // jeżeli prepare_pending_quote_retry jeszcze odroczy wycenę.
            pos.pending_quote_confirmation = Some(pending);
            return ConfirmationPoll::Pending;
        } else {
            match pending.receiver.try_recv() {
                Ok(result) => result,
                Err(oneshot::error::TryRecvError::Empty) => {
                    pos.pending_quote_confirmation = Some(pending);
                    return ConfirmationPoll::Pending;
                }
                Err(oneshot::error::TryRecvError::Closed) => Err("rpc_task_cancelled"),
            }
        };
        match result {
            Ok(confirmation)
                if confirmation.is_fresh(now_ms, self.shadow_exit_stale_after_ms()) =>
            {
                ConfirmationPoll::Ready(confirmation)
            }
            result => {
                let reason = result.err().unwrap_or("rpc_confirmation_expired");
                warn!(action_id = %pending.action.action_id, reason, "shadow_quote_confirmation_rejected");
                ConfirmationPoll::Absent
            }
        }
    }

    pub(super) fn resolve_confirmed_exit_truth(
        &self,
        confirmation: &QuoteFreshnessConfirmation,
        snapshot: &PostBuyDecisionSnapshot,
        latest: Option<&MarketSnapshot>,
        now_ms: u64,
        source: PriceTruthSource,
    ) -> Option<ShadowExitTruth> {
        let guard = snapshot.guard();
        let latest = latest?;
        if confirmation.position_id != guard.position_id()
            || confirmation.position_epoch != guard.position_epoch()
            || confirmation.requested_state_revision != guard.state_revision()
            || confirmation.remaining_quantity_raw != guard.remaining_token_amount_raw()
            || !confirmation.is_fresh(now_ms, self.shadow_exit_stale_after_ms())
            || self
                .current_canonical_state(&confirmation.canonical.base_mint)
                .as_ref()
                != Some(&confirmation.canonical)
            || source != PriceTruthSource::CanonicalAccountStateSnapshot
        {
            warn!(action_id = %confirmation.action_id, reason = "confirmation_guard_or_canonical_changed", "shadow_quote_confirmation_rejected");
            return None;
        }
        let canonical_snapshot =
            SnapshotTimeline::materialize_canonical_snapshot(&confirmation.canonical, None, 0.0);
        if latest.slot != canonical_snapshot.slot
            || latest.timestamp_ms != canonical_snapshot.timestamp_ms
            || latest.reserve_base != canonical_snapshot.reserve_base
            || latest.reserve_quote != canonical_snapshot.reserve_quote
            || latest.price_sol_per_token != canonical_snapshot.price_sol_per_token
        {
            return None;
        }
        // Wyłącznie zweryfikowane potwierdzenie zastępuje test wieku mutacji.
        // Resolver zachowuje oryginalny slot, timestamp i wiek canonical sample.
        let mut sample =
            Self::resolve_shadow_exit_sample_for_runtime(latest, now_ms, 0, source).ok()?;
        sample.evidence.detail = Some("rpc_unchanged_account_confirmed".to_string());
        PriceTruthResolver::resolve_shadow_exit(
            snapshot.entry_price_sol()?,
            guard.remaining_token_amount_raw(),
            &sample,
            0.0,
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use ghost_core::account_state_core::types::{
        AccountStateUpdate, AccountUpdateResult, UpdateSource,
    };
    use serde_json::{json, Value};
    use solana_client::rpc_request::RpcRequest;
    use tempfile::TempDir;

    struct Fixture {
        engine: MonitoringEngine,
        mint: Pubkey,
        canonical: CanonicalPoolState,
        snapshot: MarketSnapshot,
        account: Account,
        now_ms: u64,
        dir: TempDir,
    }

    fn fixture(positive_pnl: bool, with_rpc: bool) -> Fixture {
        let mint = Pubkey::new_unique();
        let curve =
            Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &PUMP_PROGRAM).0;
        let pool = curve;
        let mut data = vec![0; 151];
        data[..8].copy_from_slice(&CURVE_DISCRIMINATOR);
        for (offset, value) in [
            (8, 2_000_000_000_000_u64),
            (16, 30_000_000_000),
            (24, 1_000_000_000_000),
            (32, 10_000_000_000),
            (40, 1_000_000_000_000_000),
        ] {
            data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let account = Account {
            data,
            owner: PUMP_PROGRAM,
            lamports: 1,
            executable: false,
            rent_epoch: 0,
        };
        let core = Arc::new(AccountStateReducer::new());
        let applied = core.apply_account_update(AccountStateUpdate {
            provider_id: Some("primary-test".to_string()),
            provider_role: Some(ghost_core::RawProviderRoleV1::PrimaryAuthority),
            pool_amm_id: pool,
            base_mint: mint,
            bonding_curve: curve,
            sol_reserves: 30_000_000_000,
            token_reserves: 2_000_000_000_000,
            slot: 100,
            write_version: Some(1),
            source_account_pubkey: Some(curve),
            source_account_owner_or_program: Some(PUMP_PROGRAM),
            account_data_len: Some(account.data.len() as u64),
            account_data_hash: Some(blake3::hash(&account.data).to_hex().to_string()),
            receive_ts_ms: current_time_ms().saturating_sub(10_000),
            receive_seq: 1,
            source: UpdateSource::GeyserAccountUpdate,
            ..AccountStateUpdate::default()
        });
        assert!(matches!(
            applied,
            AccountUpdateResult::Applied | AccountUpdateResult::PromotedFromBootstrap
        ));
        let canonical = core.get_canonical_state(&mint).expect("canonical");
        let config = PostBuyGuardianConfig {
            target_threshold: Some(50.0),
            stoploss_threshold: Some(50.0),
            wait_for_timestop: Some(30_000),
            ..PostBuyGuardianConfig::default()
        };
        let (sender, _receiver) = mpsc::channel(16);
        let mut engine =
            MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), sender).unwrap();
        engine.set_account_state_core(core);
        if with_rpc {
            engine.set_shadow_quote_confirmation_rpc(mock_rpc(&account, 101));
        }
        let dir = TempDir::new().unwrap();
        engine.set_shadow_lifecycle_log_path(Some(dir.path().join("lifecycle.jsonl")));
        let price = if positive_pnl { 0.0000145 } else { 0.0000155 };
        let registered = engine
            .register_position_with_context(
                pool,
                mint,
                curve,
                Some(price),
                Some((price * 1000.0 * 1e9) as u64),
                Some(1_000_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "candidate".to_string(),
                    entry_order_id: "entry".to_string(),
                    quote_id: "quote".to_string(),
                    slot: Some(100),
                    lane: Lane::Shadow,
                    position_id: Some("test:quiet".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: None,
                }),
            )
            .unwrap();
        let now_ms = registered.opened_at_ms + 30_001;
        let snapshot = engine.current_shadow_curve_snapshot(&mint).unwrap();
        Fixture {
            engine,
            mint,
            canonical,
            snapshot,
            account,
            now_ms,
            dir,
        }
    }

    fn mock_rpc(account: &Account, slot: u64) -> Arc<RpcClient> {
        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetAccountInfo,
            rpc_account_response(account, slot),
        );
        Arc::new(RpcClient::new_mock_with_mocks(
            "succeeds".to_string(),
            mocks,
        ))
    }

    fn rpc_account_response(account: &Account, slot: u64) -> Value {
        json!({
            "context": {"slot": slot},
            "value": { "data": [base64::engine::general_purpose::STANDARD.encode(&account.data), "base64"],
                "owner": account.owner.to_string(), "lamports": account.lamports,
                "executable": account.executable, "rentEpoch": 0 }
        })
    }

    async fn begin(f: &Fixture) {
        f.engine
            .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms)
            .await;
    }

    async fn wait_for_read(f: &Fixture) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if f.engine.positions.read()[&f.mint]
                    .pending_quote_confirmation
                    .as_ref()
                    .unwrap()
                    .task
                    .is_finished()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    }

    fn rows(f: &Fixture) -> Vec<Value> {
        std::fs::read_to_string(f.dir.path().join("lifecycle.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn quiet_curve_confirmation_closes_with_pnl_without_mutating_canonical_or_activity() {
        for positive in [true, false] {
            let mut f = fixture(positive, true);
            if positive {
                let config = super::super::super::config::HetPmV2Config {
                    enabled: true,
                    ..Default::default()
                };
                f.engine.config.het_pm_v2 = config;
                f.engine.config.time_stop_v2.enabled = true;
                f.engine.het_pm_v2 =
                    Some(EffectiveHetPmV2Config::from_guardian(&f.engine.config).unwrap());
            }
            begin(&f).await;
            let activity_before = f.engine.positions.read()[&f.mint]
                .shadow_market_activity
                .last_seen_ms;
            wait_for_read(&f).await;
            f.engine
                .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms + 500)
                .await;
            let rows = rows(&f);
            let fill = rows
                .iter()
                .find(|row| row["record_type"] == "exit_filled")
                .expect("potwierdzone wyjście");
            assert_eq!(fill["net_pnl_sol"].as_f64().unwrap() > 0.0, positive);
            assert_eq!(
                fill["quote_freshness_confirmation"]["rpc_context_slot"],
                101
            );
            assert_eq!(fill["quote_freshness_confirmation"]["canonical_slot"], 100);
            assert_eq!(fill["exit_landed_slot"], 102);
            assert_eq!(
                fill["exit_landed_slot_source"],
                "synthetic_next_slot_after_rpc_confirmation"
            );
            assert_eq!(fill["sample_timestamp_ms"], f.snapshot.timestamp_ms);
            assert_eq!(fill["truth_detail"], "rpc_unchanged_account_confirmed");
            assert_eq!(fill["inactivity_age_ms"], f.now_ms + 500 - activity_before);
            assert_eq!(f.engine.current_canonical_state(&f.mint), Some(f.canonical));
            assert!(rows
                .iter()
                .any(|row| row["record_type"] == "position_closed"));
        }
    }

    #[tokio::test]
    async fn quiet_curve_without_confirmation_still_ends_unresolved_without_pnl() {
        let f = fixture(false, false);
        begin(&f).await;
        f.engine
            .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms + 5_000)
            .await;
        let rows = rows(&f);
        assert!(!rows.iter().any(|row| row["record_type"] == "exit_filled"));
        let terminal = rows
            .iter()
            .find(|row| row["record_type"] == "position_unresolved")
            .unwrap();
        assert!(terminal.get("net_pnl_sol").is_none());
        assert_eq!(f.engine.current_canonical_state(&f.mint), Some(f.canonical));
    }

    #[tokio::test]
    async fn confirmation_rejects_wrong_owner_discriminator_bytes_slot_and_identity() {
        let f = fixture(false, true);
        begin(&f).await;
        let pending = f.engine.positions.read()[&f.mint]
            .pending_quote_confirmation
            .as_ref()
            .unwrap()
            .action
            .clone();
        let confirmation =
            QuoteFreshnessConfirmation::new(&pending, f.canonical.clone(), f.now_ms).unwrap();
        assert_eq!(confirmation.validate_account(&f.account, 101), Ok(()));
        let mut wrong = f.account.clone();
        wrong.owner = Pubkey::new_unique();
        assert_eq!(
            confirmation.validate_account(&wrong, 101),
            Err("rpc_owner_mismatch")
        );
        let mut wrong = f.account.clone();
        wrong.data[0] ^= 1;
        assert_eq!(
            confirmation.validate_account(&wrong, 101),
            Err("rpc_discriminator_mismatch")
        );
        let mut wrong = f.account.clone();
        wrong.data[100] ^= 1;
        assert_eq!(
            confirmation.validate_account(&wrong, 101),
            Err("rpc_bytes_diverge")
        );
        assert_eq!(
            confirmation.validate_account(&f.account, 99),
            Err("rpc_context_slot_too_old")
        );
        let mut head_bound = confirmation.clone();
        head_bound.min_context_slot = 105;
        assert_eq!(
            head_bound.validate_account(&f.account, 101),
            Err("rpc_context_slot_too_old")
        );
        let mut wrong = f.canonical.clone();
        wrong.bonding_curve = Pubkey::new_unique();
        assert_eq!(
            QuoteFreshnessConfirmation::new(&pending, wrong, f.now_ms).unwrap_err(),
            "canonical_identity_mismatch"
        );
        assert!(!confirmation.is_fresh(f.now_ms + 1501, 1500));
        assert!(!confirmation.is_fresh(pending.recovery_deadline_ms, 10_000));
    }

    #[tokio::test]
    async fn confirmation_cannot_close_changed_position_or_changed_canonical() {
        for change in 0..5 {
            let f = fixture(false, true);
            begin(&f).await;
            wait_for_read(&f).await;
            {
                let mut positions = f.engine.positions.write();
                let pos = positions.get_mut(&f.mint).unwrap();
                match change {
                    0 => pos.position_epoch += 1,
                    1 => pos.state_revision += 1,
                    2 => pos.remaining_token_amount_raw -= 1,
                    3 => pos.position_id.push_str(":replacement"),
                    _ => {}
                }
            }
            if change == 4 {
                let core = f.engine.account_state_core.as_ref().unwrap();
                let applied = core.apply_account_update(AccountStateUpdate {
                    provider_id: Some("primary-test".to_string()),
                    provider_role: Some(ghost_core::RawProviderRoleV1::PrimaryAuthority),
                    pool_amm_id: f.canonical.pool_amm_id,
                    base_mint: f.mint,
                    bonding_curve: f.canonical.bonding_curve,
                    sol_reserves: f.canonical.virtual_sol_reserves + 1,
                    token_reserves: f.canonical.virtual_token_reserves,
                    slot: 101,
                    write_version: Some(2),
                    source_account_pubkey: f.canonical.source_account_pubkey,
                    source_account_owner_or_program: f.canonical.source_account_owner_or_program,
                    account_data_len: f.canonical.account_data_len,
                    account_data_hash: Some("ab".repeat(32)),
                    receive_ts_ms: f.canonical.last_update_ts_ms + 1,
                    receive_seq: 2,
                    source: UpdateSource::GeyserAccountUpdate,
                    ..AccountStateUpdate::default()
                });
                assert_eq!(applied, AccountUpdateResult::Applied);
            }
            f.engine
                .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms + 500)
                .await;
            assert!(
                !rows(&f)
                    .iter()
                    .any(|row| row["record_type"] == "exit_filled"),
                "change={change}"
            );
        }
    }

    #[tokio::test]
    async fn confirmation_survives_tick_before_quote_retry_interval() {
        let f = fixture(false, true);
        begin(&f).await;
        wait_for_read(&f).await;
        f.engine
            .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms + 499)
            .await;
        assert!(!rows(&f)
            .iter()
            .any(|row| row["record_type"] == "exit_filled"));
        f.engine
            .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms + 999)
            .await;
        let rows = rows(&f);
        let fill = rows
            .iter()
            .find(|row| row["record_type"] == "exit_filled")
            .expect("wczesny tick musi zachować potwierdzenie do dopuszczonej próby quote");
        assert!(fill["quote_freshness_confirmation"].is_object());
        assert_eq!(
            fill["quote_freshness_confirmation"]["requested_at_ms"],
            f.now_ms
        );
        assert_eq!(f.engine.current_canonical_state(&f.mint), Some(f.canonical));
    }

    #[tokio::test]
    async fn confirmation_deadline_blocks_late_response() {
        let f = fixture(false, true);
        begin(&f).await;
        wait_for_read(&f).await;
        f.engine
            .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms + 5_000)
            .await;
        assert!(!rows(&f)
            .iter()
            .any(|row| row["record_type"] == "exit_filled"));
        assert!(rows(&f)
            .iter()
            .any(|row| row["record_type"] == "position_unresolved"));
    }

    #[tokio::test]
    async fn confirmation_transport_timeout_is_bounded_and_does_not_block_ticks() {
        let mut f = fixture(false, false);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_connection, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        f.engine
            .set_shadow_quote_confirmation_rpc(Arc::new(RpcClient::new(format!(
                "http://{address}"
            ))));
        tokio::time::timeout(Duration::from_millis(200), begin(&f))
            .await
            .expect("tick nie czeka na HTTP");
        let (task_id, revision, action) = {
            let positions = f.engine.positions.read();
            let pos = &positions[&f.mint];
            let pending = pos.pending_quote_confirmation.as_ref().unwrap();
            (
                pending.task.id(),
                pos.state_revision,
                pending.action.clone(),
            )
        };
        f.engine.start_quote_confirmation(&action, f.now_ms + 100);
        tokio::time::timeout(
            Duration::from_millis(200),
            f.engine
                .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms + 500),
        )
        .await
        .expect("kolejny tick nie czeka na HTTP");
        {
            let positions = f.engine.positions.read();
            let pos = &positions[&f.mint];
            assert_eq!(pos.state_revision, revision);
            assert_eq!(
                pos.pending_quote_confirmation.as_ref().unwrap().task.id(),
                task_id
            );
        }
        wait_for_read(&f).await;
        let result = f
            .engine
            .positions
            .write()
            .get_mut(&f.mint)
            .unwrap()
            .pending_quote_confirmation
            .as_mut()
            .unwrap()
            .receiver
            .try_recv()
            .unwrap();
        assert_eq!(result.unwrap_err(), "rpc_timeout");
        f.engine
            .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms + 5_000)
            .await;
        assert!(!rows(&f)
            .iter()
            .any(|row| row["record_type"] == "exit_filled"));
        server.abort();
    }

    #[tokio::test]
    async fn removing_position_cancels_pending_confirmation() {
        let mut f = fixture(false, false);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        f.engine
            .set_shadow_quote_confirmation_rpc(Arc::new(RpcClient::new(format!(
                "http://{}",
                listener.local_addr().unwrap()
            ))));
        begin(&f).await;
        let abort = f.engine.positions.read()[&f.mint]
            .pending_quote_confirmation
            .as_ref()
            .unwrap()
            .task
            .abort_handle();
        f.engine.remove_all_positions_administratively();
        tokio::task::yield_now().await;
        assert!(abort.is_finished());
        assert_eq!(f.engine.active_position_count(), 0);
    }

    #[tokio::test]
    async fn confirmation_http_requests_exact_curve_base64_processed_and_stream_head_slot() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut f = fixture(false, false);
        let other = fixture(false, false);
        let state = other.canonical;
        let applied = f
            .engine
            .account_state_core
            .as_ref()
            .unwrap()
            .apply_account_update(AccountStateUpdate {
                provider_id: Some("primary-test".to_string()),
                provider_role: Some(ghost_core::RawProviderRoleV1::PrimaryAuthority),
                base_mint: state.base_mint,
                pool_amm_id: state.pool_amm_id,
                bonding_curve: state.bonding_curve,
                sol_reserves: state.virtual_sol_reserves,
                token_reserves: state.virtual_token_reserves,
                source_account_pubkey: state.source_account_pubkey,
                source_account_owner_or_program: state.source_account_owner_or_program,
                account_data_len: state.account_data_len,
                account_data_hash: state.account_data_hash,
                slot: 105,
                write_version: Some(1),
                receive_ts_ms: f.now_ms,
                receive_seq: 2,
                source: UpdateSource::GeyserAccountUpdate,
                ..AccountStateUpdate::default()
            });
        assert!(matches!(
            applied,
            AccountUpdateResult::Applied | AccountUpdateResult::PromotedFromBootstrap
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = rpc_account_response(&f.account, 106);
        let curve_address = f.canonical.bonding_curve.to_string();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let (body_start, body_len) = loop {
                let mut bytes = [0; 2048];
                let count = stream.read(&mut bytes).await.unwrap();
                assert!(count > 0 && buffer.len() < 8192);
                buffer.extend_from_slice(&bytes[..count]);
                if let Some(end) = buffer.windows(4).position(|s| s == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&buffer[..end]).unwrap().to_lowercase();
                    let len: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    break (end + 4, len);
                }
            };
            while buffer.len() < body_start + body_len {
                let mut bytes = [0; 2048];
                let count = stream.read(&mut bytes).await.unwrap();
                assert!(count > 0 && buffer.len() < 8192);
                buffer.extend_from_slice(&bytes[..count]);
            }
            let request: Value =
                serde_json::from_slice(&buffer[body_start..body_start + body_len]).unwrap();
            assert_eq!(request["method"], "getAccountInfo");
            assert_eq!(request["params"][0], curve_address);
            assert_eq!(request["params"][1]["encoding"], "base64");
            assert_eq!(request["params"][1]["commitment"], "processed");
            assert_eq!(request["params"][1]["minContextSlot"], 105);
            let body = json!({"jsonrpc":"2.0", "id":request["id"], "result":response}).to_string();
            let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(body.as_bytes()).await.unwrap();
        });
        f.engine
            .set_shadow_quote_confirmation_rpc(Arc::new(RpcClient::new(format!(
                "http://{address}"
            ))));
        begin(&f).await;
        wait_for_read(&f).await;
        server.await.unwrap();
        f.engine
            .run_shadow_runtime_tick(&f.mint, Some(&f.snapshot), f.now_ms + 500)
            .await;
        let rows = rows(&f);
        let fill = rows
            .iter()
            .find(|row| row["record_type"] == "exit_filled")
            .unwrap();
        assert_eq!(
            fill["quote_freshness_confirmation"]["min_context_slot"],
            105
        );
        assert_eq!(
            fill["quote_freshness_confirmation"]["rpc_context_slot"],
            106
        );
        assert_eq!(f.engine.current_canonical_state(&f.mint), Some(f.canonical));
    }
}
