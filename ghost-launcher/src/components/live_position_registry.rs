use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RegistryState {
    Open,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LivePositionRegistryRecord {
    recorded_at_ms: u64,
    state: RegistryState,
    base_mint: String,
    pool_amm_id: String,
    buy_signature: String,
    creator_pubkey: Option<String>,
    buy_landed_slot: Option<u64>,
    token_account: Option<String>,
    token_amount: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryTrackedPosition {
    pub base_mint: String,
    pub pool_amm_id: String,
    pub buy_signature: String,
    pub creator_pubkey: Option<String>,
    pub buy_landed_slot: Option<u64>,
    pub token_account: Option<String>,
    pub token_amount: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct LivePositionRegistry {
    path: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

impl LivePositionRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn record_open(
        &self,
        position: RecoveryTrackedPosition,
        recorded_at_ms: u64,
    ) -> Result<()> {
        self.append_record(LivePositionRegistryRecord {
            recorded_at_ms,
            state: RegistryState::Open,
            base_mint: position.base_mint,
            pool_amm_id: position.pool_amm_id,
            buy_signature: position.buy_signature,
            creator_pubkey: position.creator_pubkey,
            buy_landed_slot: position.buy_landed_slot,
            token_account: position.token_account,
            token_amount: position.token_amount,
        })
        .await
    }

    pub async fn record_closed(
        &self,
        base_mint: &str,
        pool_amm_id: &str,
        buy_signature: &str,
        recorded_at_ms: u64,
    ) -> Result<()> {
        self.append_record(LivePositionRegistryRecord {
            recorded_at_ms,
            state: RegistryState::Closed,
            base_mint: base_mint.to_string(),
            pool_amm_id: pool_amm_id.to_string(),
            buy_signature: buy_signature.to_string(),
            creator_pubkey: None,
            buy_landed_slot: None,
            token_account: None,
            token_amount: None,
        })
        .await
    }

    pub async fn load_open_positions(&self) -> Result<HashMap<String, RecoveryTrackedPosition>> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(&self.path).await.with_context(|| {
            format!(
                "failed to read live position registry {}",
                self.path.display()
            )
        })?;

        let mut open_positions = HashMap::new();
        for (line_number, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let record: LivePositionRegistryRecord =
                serde_json::from_str(trimmed).with_context(|| {
                    format!(
                        "invalid live position registry json at {}:{}",
                        self.path.display(),
                        line_number + 1
                    )
                })?;
            match record.state {
                RegistryState::Open => {
                    open_positions.insert(
                        record.base_mint.clone(),
                        RecoveryTrackedPosition {
                            base_mint: record.base_mint,
                            pool_amm_id: record.pool_amm_id,
                            buy_signature: record.buy_signature,
                            creator_pubkey: record.creator_pubkey,
                            buy_landed_slot: record.buy_landed_slot,
                            token_account: record.token_account,
                            token_amount: record.token_amount,
                        },
                    );
                }
                RegistryState::Closed => {
                    open_positions.remove(&record.base_mint);
                }
            }
        }
        Ok(open_positions)
    }

    async fn append_record(&self, record: LivePositionRegistryRecord) -> Result<()> {
        let _guard = self.write_lock.lock().await;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .with_context(|| format!("failed to open {}", self.path.display()))?;
        let line = serde_json::to_string(&record)
            .context("failed to serialize live position registry record")?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        file.sync_all().await?;
        Ok(())
    }
}
