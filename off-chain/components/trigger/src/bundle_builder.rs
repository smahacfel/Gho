//! Bundle Builder for Jito Integration
//!
//! This module provides high-level utilities for building Jito bundles
//! from Ghost transactions and InitializePool transactions.

use crate::config::BundleConfig;
use crate::errors::{Result, TriggerError};
use crate::jito_client::{BundleDiagnostics, BundleState, JitoClient};
use solana_sdk::{hash::Hash, transaction::VersionedTransaction};
use std::time::Duration;
use tracing::info;

const BUNDLE_CONFIRMATION_TIMEOUT_MS: u64 = 1_500;
const BUNDLE_CONFIRMATION_POLL_INTERVAL_MS: u64 = 200;

/// High-level bundle builder that integrates transaction building with Jito submission.
pub struct BundleBuilder {
    /// Jito client for bundle submission.
    jito_client: JitoClient,
}

impl BundleBuilder {
    /// Create a new bundle builder.
    pub fn new(jito_client: JitoClient) -> Self {
        Self { jito_client }
    }

    /// Build and submit a bundle from InitializePool TX and Ghost TXs.
    ///
    /// The SELL path stays blocked until Jito confirms that the submitted bundle
    /// was actually accepted on-chain.
    pub async fn build_and_submit(
        &self,
        init_pool_tx: VersionedTransaction,
        ghost_txs: Vec<VersionedTransaction>,
        transaction_value: u64,
        priority: f64,
        recent_blockhash: Hash,
    ) -> Result<(solana_sdk::signature::Signature, BundleDiagnostics)> {
        info!(
            "Building Jito bundle: {} Ghost TX(s), value={} lamports, priority={:.2}",
            ghost_txs.len(),
            transaction_value,
            priority
        );

        let bundle = self.jito_client.build_bundle(
            init_pool_tx,
            ghost_txs,
            transaction_value,
            priority,
            recent_blockhash,
            None,
        )?;

        let diagnostics = self
            .jito_client
            .create_diagnostics(&bundle, transaction_value, priority);
        self.jito_client.log_diagnostics(&diagnostics);

        let submission = self
            .jito_client
            .submit_bundle_with_redundancy_receipt(bundle)
            .await?;

        if self.jito_client.is_dry_run() {
            info!(
                "Bundle submitted successfully in dry-run mode: {}",
                submission.signature
            );
            return Ok((submission.signature, diagnostics));
        }

        let status = self
            .jito_client
            .wait_for_bundle_status_with_timeout(
                &submission.bundle_uuid,
                &submission.submit_endpoint,
                Duration::from_millis(BUNDLE_CONFIRMATION_TIMEOUT_MS),
                Duration::from_millis(BUNDLE_CONFIRMATION_POLL_INTERVAL_MS),
            )
            .await?;

        match status.status {
            BundleState::Accepted => {
                info!(
                    "Bundle accepted: sig={}, bundle_uuid={}, landed_slot={:?}",
                    submission.signature, submission.bundle_uuid, status.landed_slot
                );
                Ok((submission.signature, diagnostics))
            }
            BundleState::Rejected | BundleState::Expired => {
                Err(TriggerError::JitoBundleError(format!(
                    "Bundle {} was not accepted by Jito (status: {:?})",
                    submission.bundle_uuid, status.status
                )))
            }
            BundleState::Pending => Err(TriggerError::JitoBundleError(format!(
                "Bundle {} remained pending after {}ms",
                submission.bundle_uuid, BUNDLE_CONFIRMATION_TIMEOUT_MS
            ))),
        }
    }

    /// Convenience method for single Ghost transaction scenarios.
    pub async fn build_and_submit_single(
        &self,
        init_pool_tx: VersionedTransaction,
        ghost_tx: VersionedTransaction,
        transaction_value: u64,
        priority: f64,
        recent_blockhash: Hash,
    ) -> Result<(solana_sdk::signature::Signature, BundleDiagnostics)> {
        self.build_and_submit(
            init_pool_tx,
            vec![ghost_tx],
            transaction_value,
            priority,
            recent_blockhash,
        )
        .await
    }

    /// Get the bundle configuration.
    pub fn bundle_config(&self) -> &BundleConfig {
        self.jito_client.bundle_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BundleConfig, RedundancyPolicy};
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;

    #[tokio::test]
    async fn test_bundle_builder_single_tx() {
        let config = BundleConfig::default();
        let mut jito_client = JitoClient::new("https://test.jito.wtf/api/v1", config);
        jito_client.set_dry_run(true);
        let builder = BundleBuilder::new(jito_client);

        let init_tx = create_dummy_transaction();
        let ghost_tx = create_dummy_transaction();

        let result = builder
            .build_and_submit_single(init_tx, ghost_tx, 1_000_000_000, 0.5, Hash::default())
            .await;

        assert!(result.is_ok());
        let (_signature, diagnostics) = result.unwrap();
        assert_eq!(diagnostics.transaction_count, 2);
        assert_eq!(diagnostics.tip_lamports, 35_000_000);
    }

    #[tokio::test]
    async fn test_bundle_builder_multiple_tx() {
        let config = BundleConfig::default();
        let mut jito_client = JitoClient::new("https://test.jito.wtf/api/v1", config);
        jito_client.set_dry_run(true);
        let builder = BundleBuilder::new(jito_client);

        let init_tx = create_dummy_transaction();
        let ghost_txs = vec![
            create_dummy_transaction(),
            create_dummy_transaction(),
            create_dummy_transaction(),
        ];

        let result = builder
            .build_and_submit(init_tx, ghost_txs, 5_000_000_000, 0.8, Hash::default())
            .await;

        assert!(result.is_ok());
        let (_, diagnostics) = result.unwrap();
        assert_eq!(diagnostics.transaction_count, 4);
        assert_eq!(diagnostics.tip_lamports, 100_000_000);
    }

    #[tokio::test]
    async fn test_bundle_builder_with_custom_policy() {
        let mut config = BundleConfig::default();
        config.redundancy_policy = RedundancyPolicy::NPlusFive;

        let jito_client = JitoClient::new("https://test.jito.wtf/api/v1", config);
        let builder = BundleBuilder::new(jito_client);

        assert_eq!(
            builder.bundle_config().redundancy_policy,
            RedundancyPolicy::NPlusFive
        );
    }

    fn create_dummy_transaction() -> VersionedTransaction {
        use solana_sdk::message::{v0, VersionedMessage};

        let payer = Keypair::new();
        let message = v0::Message::try_compile(&payer.pubkey(), &[], &[], Hash::default()).unwrap();

        VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer]).unwrap()
    }
}
