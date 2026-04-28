//! ShadowBackend — canonical synthetic entry settlement on prepared-entry truth.

use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::{create_dir_all, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

use super::backend::*;
use crate::config::{ExecutionShadowConfig, ExecutionShadowTimingModel};
use crate::events::EventEmitter;
use tracing::error;

#[derive(Debug, Clone)]
struct PendingShadowOrder {
    attempt: ExecutionAttemptContext,
}

#[derive(Debug, Clone)]
pub struct ShadowPositionState {
    pub position_id: PositionId,
    pub order_id: OrderId,
    pub candidate_id: CandidateId,
    pub pool_amm_id: solana_sdk::pubkey::Pubkey,
    pub base_mint: solana_sdk::pubkey::Pubkey,
    pub quote_id: QuoteId,
    pub entry_time_ms: u64,
    pub entry_price: f64,
    pub size_tokens: u64,
    pub size_sol: u64,
    pub epoch_id: u64,
}

#[derive(Debug, Serialize)]
struct ShadowEntryRecord {
    timestamp: String,
    pool_id: String,
    mint_id: String,
    entry_price: f64,
    slot: Option<u64>,
    timestamp_ms: u64,
    candidate_id: String,
    order_id: String,
    position_id: String,
    position_epoch: u64,
    lane: Lane,
    amount_lamports: u64,
    min_tokens_out: u64,
    fill_qty: u64,
    quote_id: String,
    quote_ts_ms: u64,
    quote_slot: Option<u64>,
    quote_price_ref: Option<f64>,
    price_source: EntryPriceSource,
    quote_is_stale: bool,
    stale_age_ms: u64,
    stale_policy: EntryStalePolicy,
    prepared_timing_source: EntryTimingSource,
    timing_source: EntryTimingPath,
    reference_slot: Option<u64>,
    predicted_slot: Option<u64>,
    planned_settle_time_ms: u64,
    actual_settle_time_ms: u64,
    compensation_ms: u64,
    latency_ms: u64,
    status: FillStatus,
}

pub struct ShadowBackend {
    config: ExecutionShadowConfig,
    pending_orders: Arc<RwLock<HashMap<OrderId, PendingShadowOrder>>>,
    positions: Arc<RwLock<HashMap<PositionId, ShadowPositionState>>>,
    next_order_id: Arc<std::sync::atomic::AtomicU64>,
    next_position_id: Arc<std::sync::atomic::AtomicU64>,
}

impl ShadowBackend {
    pub fn new(config: ExecutionShadowConfig) -> Self {
        Self {
            config,
            pending_orders: Arc::new(RwLock::new(HashMap::new())),
            positions: Arc::new(RwLock::new(HashMap::new())),
            next_order_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            next_position_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    pub fn reserve_entry_order_id(&self) -> OrderId {
        let id = self
            .next_order_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("shadow-{}", id)
    }

    pub async fn position_count(&self) -> usize {
        self.positions.read().await.len()
    }

    pub async fn unregister_position(&self, position_id: &str) -> Option<ShadowPositionState> {
        self.positions.write().await.remove(position_id)
    }

    pub async fn submit_prepared_entry(
        &self,
        attempt: ExecutionAttemptContext,
    ) -> Result<OrderId, ExecutionError> {
        if !matches!(
            self.config.timing_model,
            ExecutionShadowTimingModel::PreparedEntryMirror
        ) {
            return Err(ExecutionError::TransactionFailed(
                "ShadowBackend requires execution.shadow.timing_model=prepared_entry_mirror"
                    .to_string(),
            ));
        }

        let order_id = attempt.prepared.order_id.clone();
        self.pending_orders
            .write()
            .await
            .insert(order_id.clone(), PendingShadowOrder { attempt });
        Ok(order_id)
    }

    async fn settle_due_order(&self, pending: PendingShadowOrder, now_ms: u64) -> FillEvent {
        let prepared = &pending.attempt.prepared;
        let latency_ms = now_ms.saturating_sub(prepared.submit_time_ms);

        if prepared.quote.is_stale
            && matches!(prepared.quote.stale_policy, EntryStalePolicy::Reject)
        {
            return FillEvent {
                order_id: prepared.order_id.clone(),
                position_id: None,
                side: OrderSide::Entry,
                status: FillStatus::Stale,
                fill_price: 0.0,
                fill_qty: 0,
                quote_id_used: prepared.quote.quote_id.clone(),
                fill_time_ms: now_ms,
                latency_ms,
                lane: Lane::Shadow,
            };
        }

        if prepared.candidate.min_tokens_out == 0 {
            return FillEvent {
                order_id: prepared.order_id.clone(),
                position_id: None,
                side: OrderSide::Entry,
                status: FillStatus::Failed,
                fill_price: 0.0,
                fill_qty: 0,
                quote_id_used: prepared.quote.quote_id.clone(),
                fill_time_ms: now_ms,
                latency_ms,
                lane: Lane::Shadow,
            };
        }

        let fill_price = prepared.quote.quote_price_ref.unwrap_or_else(|| {
            prepared.candidate.entry_amount_lamports as f64
                / prepared.candidate.min_tokens_out as f64
        });
        let fill_qty = prepared.candidate.min_tokens_out;
        let position_id = self
            .next_position_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let position_id = format!("shadow:{}:{}", prepared.candidate.base_mint, position_id);
        let position = ShadowPositionState {
            position_id: position_id.clone(),
            order_id: prepared.order_id.clone(),
            candidate_id: prepared.candidate_id().to_string(),
            pool_amm_id: prepared.candidate.pool_amm_id,
            base_mint: prepared.candidate.base_mint,
            quote_id: prepared.quote.quote_id.clone(),
            entry_time_ms: now_ms,
            entry_price: fill_price,
            size_tokens: fill_qty,
            size_sol: prepared.candidate.entry_amount_lamports,
            epoch_id: prepared.position_epoch,
        };
        let record = ShadowEntryRecord {
            timestamp: chrono::Utc::now().to_rfc3339(),
            pool_id: prepared.candidate.pool_amm_id.to_string(),
            mint_id: prepared.candidate.base_mint.to_string(),
            entry_price: fill_price,
            slot: prepared.quote.slot,
            timestamp_ms: now_ms,
            candidate_id: prepared.candidate_id().to_string(),
            order_id: prepared.order_id.clone(),
            position_id: position_id.clone(),
            position_epoch: prepared.position_epoch,
            lane: Lane::Shadow,
            amount_lamports: prepared.candidate.entry_amount_lamports,
            min_tokens_out: prepared.candidate.min_tokens_out,
            fill_qty,
            quote_id: prepared.quote.quote_id.clone(),
            quote_ts_ms: prepared.quote.quote_ts_ms,
            quote_slot: prepared.quote.slot,
            quote_price_ref: prepared.quote.quote_price_ref,
            price_source: prepared.quote.price_source,
            quote_is_stale: prepared.quote.is_stale,
            stale_age_ms: prepared.quote.stale_age_ms,
            stale_policy: prepared.quote.stale_policy,
            prepared_timing_source: prepared.timing_source,
            timing_source: pending.attempt.timing.timing_path,
            reference_slot: pending.attempt.timing.reference_slot,
            predicted_slot: pending.attempt.timing.predicted_slot,
            planned_settle_time_ms: pending.attempt.timing.planned_settle_time_ms,
            actual_settle_time_ms: now_ms,
            compensation_ms: pending.attempt.timing.compensation_ms,
            latency_ms,
            status: FillStatus::Confirmed,
        };

        if let Err(error) =
            append_jsonl_record(Path::new(&self.config.entry_log_path), &record).await
        {
            error!(
                order_id = %prepared.order_id,
                path = %self.config.entry_log_path,
                error = %error,
                "ShadowBackend failed to persist shadow_entries.jsonl"
            );
            return FillEvent {
                order_id: prepared.order_id.clone(),
                position_id: None,
                side: OrderSide::Entry,
                status: FillStatus::Failed,
                fill_price: 0.0,
                fill_qty: 0,
                quote_id_used: prepared.quote.quote_id.clone(),
                fill_time_ms: now_ms,
                latency_ms,
                lane: Lane::Shadow,
            };
        }

        self.positions
            .write()
            .await
            .insert(position_id.clone(), position);

        FillEvent {
            order_id: prepared.order_id.clone(),
            position_id: Some(position_id),
            side: OrderSide::Entry,
            status: FillStatus::Confirmed,
            fill_price,
            fill_qty,
            quote_id_used: prepared.quote.quote_id.clone(),
            fill_time_ms: now_ms,
            latency_ms,
            lane: Lane::Shadow,
        }
    }
}

#[async_trait]
impl ExecutionBackend for ShadowBackend {
    async fn submit_entry(
        &self,
        candidate: &CandidateRef,
        quote_ref: QuoteId,
        position_epoch: u64,
    ) -> Result<OrderId, ExecutionError> {
        let submit_time_ms = EventEmitter::now_ms();
        let prepared = PreparedEntryExecution {
            order_id: self.reserve_entry_order_id(),
            candidate: candidate.clone(),
            submit_time_ms,
            position_epoch,
            quote: PreparedQuoteRef {
                quote_id: quote_ref,
                quote_ts_ms: submit_time_ms,
                slot: None,
                quote_price_ref: None,
                price_source: EntryPriceSource::EffectiveFillFallback,
                is_stale: false,
                stale_age_ms: 0,
                stale_policy: self.config.stale_policy,
            },
            timing_source: EntryTimingSource::LegacyBackend,
            predicted_slot: None,
        };
        self.submit_prepared_entry(ExecutionAttemptContext::new(
            prepared,
            None,
            self.config.tx_build_compensation_ms,
        ))
        .await
    }

    async fn submit_exit(
        &self,
        _position_id: &PositionId,
        _fraction_bps: u16,
        _quote_ref: QuoteId,
        _command_ref: Option<CommandId>,
    ) -> Result<OrderId, ExecutionError> {
        Err(ExecutionError::TransactionFailed(
            "ShadowBackend exits land in PR-4".to_string(),
        ))
    }

    async fn poll_fills(&self, now_ms: u64) -> Vec<FillEvent> {
        let due_order_ids = {
            let orders = self.pending_orders.read().await;
            orders
                .iter()
                .filter(|(_, pending)| pending.attempt.timing.planned_settle_time_ms <= now_ms)
                .map(|(order_id, _)| order_id.clone())
                .collect::<Vec<_>>()
        };

        let mut due_orders = Vec::with_capacity(due_order_ids.len());
        {
            let mut orders = self.pending_orders.write().await;
            for order_id in due_order_ids {
                if let Some(pending) = orders.remove(&order_id) {
                    due_orders.push(pending);
                }
            }
        }

        let mut fills = Vec::with_capacity(due_orders.len());
        for pending in due_orders {
            fills.push(self.settle_due_order(pending, now_ms).await);
        }
        fills
    }

    fn get_execution_stress(&self, _position_id: &PositionId) -> ExecutionStressSnapshot {
        ExecutionStressSnapshot::default()
    }

    fn lane(&self) -> Lane {
        Lane::Shadow
    }
}

async fn append_jsonl_record(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_dir_all(parent).await?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    let json = serde_json::to_string(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
    file.write_all(json.as_bytes()).await?;
    file.write_all(b"\n").await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn shadow_config(path: &Path) -> ExecutionShadowConfig {
        ExecutionShadowConfig {
            entry_log_path: path.to_string_lossy().to_string(),
            timing_model: ExecutionShadowTimingModel::PreparedEntryMirror,
            ..ExecutionShadowConfig::default()
        }
    }

    fn prepared_entry(
        order_id: &str,
        timing_source: EntryTimingSource,
        is_stale: bool,
        stale_policy: EntryStalePolicy,
    ) -> PreparedEntryExecution {
        PreparedEntryExecution {
            order_id: order_id.to_string(),
            candidate: CandidateRef {
                candidate_id: "cand-shadow".to_string(),
                base_mint: solana_sdk::pubkey::Pubkey::new_unique(),
                pool_amm_id: solana_sdk::pubkey::Pubkey::new_unique(),
                entry_amount_lamports: 1_000_000,
                min_tokens_out: 50_000,
            },
            submit_time_ms: 10_000,
            position_epoch: 7,
            quote: PreparedQuoteRef {
                quote_id: "quote-shadow".to_string(),
                quote_ts_ms: 9_950,
                slot: Some(412_000),
                quote_price_ref: Some(0.02),
                price_source: EntryPriceSource::SnapshotEngine,
                is_stale,
                stale_age_ms: 50,
                stale_policy,
            },
            timing_source,
            predicted_slot: None,
        }
    }

    #[tokio::test]
    async fn shadow_submit_prepared_entry_opens_position_and_writes_log() {
        let dir = tempdir().expect("tempdir");
        let log_path = dir.path().join("shadow_entries.jsonl");
        let backend = ShadowBackend::new(shadow_config(&log_path));
        let attempt = ExecutionAttemptContext::new(
            prepared_entry(
                "shadow-1",
                EntryTimingSource::LiveJito,
                false,
                EntryStalePolicy::EmitWarning,
            ),
            Some(412_100),
            250,
        );
        let planned_settle_time_ms = attempt.timing.planned_settle_time_ms;

        backend
            .submit_prepared_entry(attempt)
            .await
            .expect("submit prepared entry");
        assert!(backend
            .poll_fills(planned_settle_time_ms.saturating_sub(1))
            .await
            .is_empty());

        let fills = backend.poll_fills(planned_settle_time_ms).await;
        assert_eq!(fills.len(), 1);
        let fill = &fills[0];
        assert_eq!(fill.status, FillStatus::Confirmed);
        assert_eq!(fill.lane, Lane::Shadow);
        assert!(fill.position_id.is_some());
        assert_eq!(backend.position_count().await, 1);

        let content = tokio::fs::read_to_string(&log_path)
            .await
            .expect("read shadow log");
        let first_line = content.lines().next().expect("shadow log line");
        let record: serde_json::Value =
            serde_json::from_str(first_line).expect("parse shadow record");
        for field in [
            "timestamp",
            "pool_id",
            "mint_id",
            "entry_price",
            "slot",
            "timestamp_ms",
            "candidate_id",
            "order_id",
            "quote_id",
            "timing_source",
        ] {
            assert!(record.get(field).is_some(), "missing field: {field}");
        }
        assert_eq!(record["timing_source"], "jito_batch_path");
    }

    #[tokio::test]
    async fn shadow_rejects_stale_quote_when_policy_is_reject() {
        let dir = tempdir().expect("tempdir");
        let log_path = dir.path().join("shadow_entries.jsonl");
        let backend = ShadowBackend::new(shadow_config(&log_path));
        let attempt = ExecutionAttemptContext::new(
            prepared_entry(
                "shadow-2",
                EntryTimingSource::LiveStandard,
                true,
                EntryStalePolicy::Reject,
            ),
            Some(412_100),
            250,
        );
        let planned_settle_time_ms = attempt.timing.planned_settle_time_ms;

        backend
            .submit_prepared_entry(attempt)
            .await
            .expect("submit stale entry");

        let fills = backend.poll_fills(planned_settle_time_ms).await;
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].status, FillStatus::Stale);
        assert!(fills[0].position_id.is_none());
        assert_eq!(backend.position_count().await, 0);
        assert!(
            !log_path.exists(),
            "stale-rejected fills must not open/log synthetic shadow positions"
        );
    }
}
