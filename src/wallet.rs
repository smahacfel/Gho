//! Wallet management module

use anyhow::{Context, Result};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Wallet manager for handling keypairs and signing
/// Enhanced with authority rotation support (Security Enhancement 2)
pub struct WalletManager {
    keypair: Arc<Keypair>,
    /// Shared authority pool for nonce manager integration
    authority_pool: Arc<RwLock<Vec<Pubkey>>>,
}

impl WalletManager {
    /// Create a new wallet manager from a keypair file
    pub fn from_file(path: &str) -> Result<Self> {
        let keypair_bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read keypair file: {}", path))?;

        let keypair = if keypair_bytes.len() == 64 {
            // Raw bytes format - validate before conversion
            if keypair_bytes.iter().all(|&b| b == 0) {
                anyhow::bail!("Invalid keypair: all-zero key rejected");
            }
            Keypair::try_from(keypair_bytes.as_slice()).context("Invalid keypair bytes")?
        } else {
            // JSON format
            let json: Vec<u8> =
                serde_json::from_slice(&keypair_bytes).context("Failed to parse keypair JSON")?;
            if json.len() != 64 {
                anyhow::bail!(
                    "Invalid keypair length: expected 64 bytes, got {}",
                    json.len()
                );
            }
            if json.iter().all(|&b| b == 0) {
                anyhow::bail!("Invalid keypair: all-zero key rejected");
            }
            Keypair::try_from(json.as_slice()).context("Invalid keypair from JSON")?
        };

        Ok(Self {
            keypair: Arc::new(keypair),
            authority_pool: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Create a new wallet manager from a keypair
    pub fn from_keypair(keypair: Keypair) -> Self {
        Self {
            keypair: Arc::new(keypair),
            authority_pool: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get the public key
    pub fn pubkey(&self) -> Pubkey {
        self.keypair.pubkey()
    }

    /// Get a reference to the keypair
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    /// Clone the keypair (for signing operations)
    pub fn keypair_cloned(&self) -> Keypair {
        Keypair::try_from(self.keypair.to_bytes().as_slice()).expect("Valid keypair")
    }

    /// Get an Arc reference to the keypair (for use with libraries expecting Arc<Keypair>)
    pub fn keypair_arc(&self) -> Arc<Keypair> {
        Arc::clone(&self.keypair)
    }

    /// Add authority to shared pool (Security Enhancement 2)
    /// Syncs with nonce manager authority rotation
    pub async fn add_authority(&self, authority: Pubkey) {
        let mut pool = self.authority_pool.write().await;
        if !pool.contains(&authority) {
            pool.push(authority);
        }
    }

    /// Remove authority from shared pool
    pub async fn remove_authority(&self, authority: &Pubkey) {
        let mut pool = self.authority_pool.write().await;
        pool.retain(|a| a != authority);
    }

    /// Get all authorities in pool
    pub async fn get_authorities(&self) -> Vec<Pubkey> {
        self.authority_pool.read().await.clone()
    }

    /// Get shared authority pool reference for nonce manager integration
    pub fn authority_pool_ref(&self) -> Arc<RwLock<Vec<Pubkey>>> {
        Arc::clone(&self.authority_pool)
    }
}

impl Clone for WalletManager {
    fn clone(&self) -> Self {
        Self {
            keypair: Arc::clone(&self.keypair),
            authority_pool: Arc::clone(&self.authority_pool),
        }
    }
}
