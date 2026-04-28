//! StorageActor - Actor wrapper for DecisionLedger
//!
//! This actor wraps the DecisionLedger component and handles storage operations.

use super::messages::{GetStorageStats, RecordDecision, StorageStats, UpdateOutcome};
use crate::oracle::decision_ledger::DecisionLedger;
use crate::oracle::storage::LedgerStorage;
use actix::prelude::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

/// Actor that manages the DecisionLedger component
pub struct StorageActor {
    storage: Arc<dyn LedgerStorage>,
    decision_sender: mpsc::Sender<crate::oracle::types::TransactionRecord>,
    outcome_sender: mpsc::Sender<(
        String,
        crate::oracle::types::Outcome,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<u64>,
        bool,
    )>,
}

impl StorageActor {
    /// Create a new StorageActor
    pub async fn new() -> Result<Self, anyhow::Error> {
        // Create channels for the DecisionLedger
        let (decision_sender, decision_receiver) = mpsc::channel(100);
        let (outcome_sender, outcome_receiver) = mpsc::channel(100);

        // Create the DecisionLedger
        let ledger = DecisionLedger::new(decision_receiver, outcome_receiver).await?;
        let storage = ledger.get_storage();

        // Spawn the ledger processing task
        tokio::spawn(async move {
            ledger.run().await;
        });

        Ok(Self {
            storage,
            decision_sender,
            outcome_sender,
        })
    }

    /// Create a new StorageActor with normalized storage
    pub async fn new_normalized() -> Result<Self, anyhow::Error> {
        // Create channels for the DecisionLedger
        let (decision_sender, decision_receiver) = mpsc::channel(100);
        let (outcome_sender, outcome_receiver) = mpsc::channel(100);

        // Create the DecisionLedger with normalized storage
        let ledger = DecisionLedger::new_normalized(decision_receiver, outcome_receiver).await?;
        let storage = ledger.get_storage();

        // Spawn the ledger processing task
        tokio::spawn(async move {
            ledger.run().await;
        });

        Ok(Self {
            storage,
            decision_sender,
            outcome_sender,
        })
    }

    /// Get a reference to the storage for other components
    pub fn get_storage(&self) -> Arc<dyn LedgerStorage> {
        Arc::clone(&self.storage)
    }
}

impl Actor for StorageActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("StorageActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("StorageActor stopped");
    }
}

// Handle RecordDecision messages
impl Handler<RecordDecision> for StorageActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: RecordDecision, _ctx: &mut Context<Self>) -> Self::Result {
        let sender = self.decision_sender.clone();

        Box::pin(
            async move {
                sender
                    .send(msg.record)
                    .await
                    .map_err(|e| format!("Failed to send decision record: {}", e))?;
                Ok(())
            }
            .into_actor(self),
        )
    }
}

// Handle UpdateOutcome messages
impl Handler<UpdateOutcome> for StorageActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: UpdateOutcome, _ctx: &mut Context<Self>) -> Self::Result {
        let sender = self.outcome_sender.clone();

        Box::pin(
            async move {
                let update = (
                    msg.signature,
                    msg.outcome,
                    msg.buy_price,
                    msg.sell_price,
                    msg.sol_spent,
                    msg.sol_received,
                    msg.evaluated_at,
                    msg.is_verified,
                );

                sender
                    .send(update)
                    .await
                    .map_err(|e| format!("Failed to send outcome update: {}", e))?;
                Ok(())
            }
            .into_actor(self),
        )
    }
}

// Handle GetStorageStats messages
impl Handler<GetStorageStats> for StorageActor {
    type Result = ResponseActFuture<Self, StorageStats>;

    fn handle(&mut self, _msg: GetStorageStats, _ctx: &mut Context<Self>) -> Self::Result {
        let storage = Arc::clone(&self.storage);

        Box::pin(
            async move {
                // Query storage for statistics
                let total_decisions = storage.get_record_count().await.unwrap_or(0) as u64;
                // For pending outcomes, we'd need a specific query - for now use 0
                let pending_outcomes = 0u64;

                StorageStats {
                    total_decisions,
                    pending_outcomes,
                }
            }
            .into_actor(self),
        )
    }
}
