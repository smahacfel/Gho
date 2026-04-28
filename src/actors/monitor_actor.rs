//! MonitorActor - Actor wrapper for TransactionMonitor
//!
//! This actor wraps the TransactionMonitor component and handles transaction monitoring.

use super::messages::{GetMonitorStats, MonitorStats, MonitorTransaction};
use crate::oracle::storage::LedgerStorage;
use crate::oracle::transaction_monitor::{MonitoredTransaction, TransactionMonitor};
use crate::types::Pubkey;
use actix::prelude::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

/// Actor that manages the TransactionMonitor component
pub struct MonitorActor {
    tx_sender: mpsc::Sender<MonitoredTransaction>,
    storage: Arc<dyn LedgerStorage>,
}

impl MonitorActor {
    /// Create a new MonitorActor
    pub fn new(
        storage: Arc<dyn LedgerStorage>,
        update_sender: mpsc::Sender<(
            String,
            crate::oracle::types::Outcome,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<u64>,
            bool,
        )>,
        rpc_url: String,
        wallet_pubkey: Pubkey,
        check_interval_ms: u64,
    ) -> Self {
        let (tx_sender, tx_receiver) = mpsc::channel(100);

        // Create RPC client
        let rpc_client = Arc::new(RpcClient::new(rpc_url));

        // Create the TransactionMonitor
        let monitor = TransactionMonitor::new(
            Arc::clone(&storage),
            update_sender,
            check_interval_ms,
            rpc_client,
            wallet_pubkey,
        );

        // Spawn the monitor processing task
        tokio::spawn(async move {
            monitor.run(tx_receiver).await;
        });

        Self { tx_sender, storage }
    }
}

impl Actor for MonitorActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("MonitorActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("MonitorActor stopped");
    }
}

// Handle MonitorTransaction messages
impl Handler<MonitorTransaction> for MonitorActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: MonitorTransaction, _ctx: &mut Context<Self>) -> Self::Result {
        let sender = self.tx_sender.clone();

        Box::pin(
            async move {
                sender
                    .send(msg.transaction)
                    .await
                    .map_err(|e| format!("Failed to send transaction to monitor: {}", e))?;
                Ok(())
            }
            .into_actor(self),
        )
    }
}

// Handle GetMonitorStats messages
impl Handler<GetMonitorStats> for MonitorActor {
    type Result = ResponseActFuture<Self, MonitorStats>;

    fn handle(&mut self, _msg: GetMonitorStats, _ctx: &mut Context<Self>) -> Self::Result {
        let storage = Arc::clone(&self.storage);

        Box::pin(
            async move {
                // Query storage for monitoring statistics
                let active_transactions = storage
                    .get_pending_monitoring_transactions()
                    .await
                    .map(|txs| txs.len())
                    .unwrap_or(0);

                let completed_transactions = 0u64; // Would need to track this separately

                MonitorStats {
                    active_transactions,
                    completed_transactions,
                }
            }
            .into_actor(self),
        )
    }
}
