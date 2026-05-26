use dashmap::{mapref::entry::Entry, DashMap};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solana_sdk::pubkey::Pubkey;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExecutionAccountRole {
    BondingCurveV2,
    CreatorVault,
    UserAta,
    AssociatedBondingCurve,
    Payer,
    Other(String),
}

impl ExecutionAccountRole {
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::BondingCurveV2 => "bonding_curve_v2".to_string(),
            Self::CreatorVault => "creator_vault".to_string(),
            Self::UserAta => "user_ata".to_string(),
            Self::AssociatedBondingCurve => "associated_bonding_curve".to_string(),
            Self::Payer => "payer".to_string(),
            Self::Other(name) => format!("other:{name}"),
        }
    }

    #[must_use]
    pub fn from_label(label: &str) -> Self {
        match label {
            "bonding_curve_v2" => Self::BondingCurveV2,
            "creator_vault" => Self::CreatorVault,
            "user_ata" => Self::UserAta,
            "associated_bonding_curve" => Self::AssociatedBondingCurve,
            "payer" => Self::Payer,
            other => Self::Other(other.strip_prefix("other:").unwrap_or(other).to_string()),
        }
    }
}

impl Serialize for ExecutionAccountRole {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.label())
    }
}

impl<'de> Deserialize<'de> for ExecutionAccountRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let label = String::deserialize(deserializer)?;
        Ok(Self::from_label(&label))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionAccountEvidenceSource {
    ObservedTxMeta,
    ExactWatchRegistered,
    ExactWatchSubscribeIncluded,
    YellowstoneAccountUpdate,
    RpcHydration,
    RpcPrecheck,
    ManifestPrecheck,
}

impl ExecutionAccountEvidenceSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ObservedTxMeta => "observed_tx_meta",
            Self::ExactWatchRegistered => "exact_watch_registered",
            Self::ExactWatchSubscribeIncluded => "exact_watch_subscribe_included",
            Self::YellowstoneAccountUpdate => "yellowstone_account_update",
            Self::RpcHydration => "rpc_hydration",
            Self::RpcPrecheck => "rpc_precheck",
            Self::ManifestPrecheck => "manifest_precheck",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionAccountEvidenceStatus {
    DiscoveryHint,
    SubscriptionRequested,
    SubscribeIncluded,
    AccountUpdateReceived,
    RpcReady,
    RpcMissing,
    PrecheckReady,
    PrecheckMissing,
    DecodeFailed,
    Unmapped,
}

impl ExecutionAccountEvidenceStatus {
    #[must_use]
    pub const fn precedence(self) -> u8 {
        match self {
            Self::Unmapped => 0,
            Self::DiscoveryHint => 10,
            Self::SubscriptionRequested => 20,
            Self::SubscribeIncluded => 30,
            Self::AccountUpdateReceived | Self::DecodeFailed => 40,
            Self::RpcReady | Self::RpcMissing => 50,
            Self::PrecheckReady | Self::PrecheckMissing => 60,
        }
    }

    #[must_use]
    pub const fn is_positive_evidence(self) -> bool {
        matches!(
            self,
            Self::DiscoveryHint
                | Self::SubscriptionRequested
                | Self::SubscribeIncluded
                | Self::AccountUpdateReceived
                | Self::RpcReady
                | Self::PrecheckReady
        )
    }

    #[must_use]
    pub const fn is_missing_or_negative(self) -> bool {
        matches!(
            self,
            Self::RpcMissing | Self::PrecheckMissing | Self::DecodeFailed | Self::Unmapped
        )
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DiscoveryHint => "discovery_hint",
            Self::SubscriptionRequested => "subscription_requested",
            Self::SubscribeIncluded => "subscribe_included",
            Self::AccountUpdateReceived => "account_update_received",
            Self::RpcReady => "rpc_ready",
            Self::RpcMissing => "rpc_missing",
            Self::PrecheckReady => "precheck_ready",
            Self::PrecheckMissing => "precheck_missing",
            Self::DecodeFailed => "decode_failed",
            Self::Unmapped => "unmapped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionAccountEvidence {
    pub role: ExecutionAccountRole,
    pub account_pubkey: Pubkey,
    pub base_mint: Option<Pubkey>,
    pub pool_id: Option<Pubkey>,
    pub canonical_bonding_curve: Option<Pubkey>,

    pub source: ExecutionAccountEvidenceSource,
    pub status: ExecutionAccountEvidenceStatus,

    pub slot: Option<u64>,
    pub context_slot: Option<u64>,
    pub write_version: Option<u64>,

    pub owner: Option<Pubkey>,
    pub data_len: Option<u64>,

    pub tx_signature: Option<String>,
    pub observed_instruction_index: Option<u32>,
    pub observed_account_position: Option<u32>,
    pub provenance_status: Option<String>,

    pub detected_at_ms: u64,
    pub received_at_ms: u64,
    pub evidence_ready: bool,
    pub reason: Option<String>,
}

impl ExecutionAccountEvidence {
    #[must_use]
    pub fn exact_key(&self) -> (ExecutionAccountRole, Pubkey) {
        (self.role.clone(), self.account_pubkey)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionAccountEvidenceConflict {
    pub positive_status: ExecutionAccountEvidenceStatus,
    pub positive_source: ExecutionAccountEvidenceSource,
    pub negative_status: ExecutionAccountEvidenceStatus,
    pub negative_source: ExecutionAccountEvidenceSource,
    pub reason: String,
}

impl ExecutionAccountEvidenceConflict {
    fn from_record(record: &ExecutionAccountEvidenceRecord) -> Option<Self> {
        let positive = record.best_positive.as_ref()?;
        let negative = record.latest_negative.as_ref()?;
        Some(Self {
            positive_status: positive.status,
            positive_source: positive.source,
            negative_status: negative.status,
            negative_source: negative.source,
            reason: format!(
                "positive_{}_conflicts_with_negative_{}",
                positive.status.as_str(),
                negative.status.as_str()
            ),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionAccountEvidenceRecord {
    pub best_positive: Option<ExecutionAccountEvidence>,
    pub latest_negative: Option<ExecutionAccountEvidence>,
    pub latest: ExecutionAccountEvidence,
    pub conflict: Option<ExecutionAccountEvidenceConflict>,
}

impl ExecutionAccountEvidenceRecord {
    #[must_use]
    pub fn new(evidence: ExecutionAccountEvidence) -> Self {
        let mut record = Self {
            best_positive: None,
            latest_negative: None,
            latest: evidence.clone(),
            conflict: None,
        };
        record.merge(evidence);
        record
    }

    fn merge(&mut self, evidence: ExecutionAccountEvidence) {
        if evidence.status.is_positive_evidence()
            && self
                .best_positive
                .as_ref()
                .map(|current| should_replace_best_positive(&evidence, current))
                .unwrap_or(true)
        {
            self.best_positive = Some(evidence.clone());
        }

        if evidence.status.is_missing_or_negative() {
            self.latest_negative = Some(evidence.clone());
        }

        self.latest = evidence;
        self.conflict = ExecutionAccountEvidenceConflict::from_record(self);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpsertExecutionAccountEvidenceOutcome {
    Inserted,
    Updated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpsertExecutionAccountEvidenceResult {
    pub outcome: UpsertExecutionAccountEvidenceOutcome,
    pub role: ExecutionAccountRole,
    pub account_pubkey: Pubkey,
    pub latest_status: ExecutionAccountEvidenceStatus,
    pub best_positive_status: Option<ExecutionAccountEvidenceStatus>,
    pub latest_negative_status: Option<ExecutionAccountEvidenceStatus>,
    pub conflict: Option<ExecutionAccountEvidenceConflict>,
}

impl UpsertExecutionAccountEvidenceResult {
    fn from_record(
        outcome: UpsertExecutionAccountEvidenceOutcome,
        role: ExecutionAccountRole,
        account_pubkey: Pubkey,
        record: &ExecutionAccountEvidenceRecord,
    ) -> Self {
        Self {
            outcome,
            role,
            account_pubkey,
            latest_status: record.latest.status,
            best_positive_status: record
                .best_positive
                .as_ref()
                .map(|evidence| evidence.status),
            latest_negative_status: record
                .latest_negative
                .as_ref()
                .map(|evidence| evidence.status),
            conflict: record.conflict.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionAccountEvidenceSnapshotCounts {
    pub total_records: usize,
    pub positive_records: usize,
    pub negative_records: usize,
    pub conflict_records: usize,
    pub evidence_ready_records: usize,
    pub latest_status_counts: BTreeMap<String, usize>,
    pub latest_source_counts: BTreeMap<String, usize>,
}

#[derive(Default)]
pub struct ExecutionAccountEvidenceStore {
    by_role_pubkey: DashMap<(ExecutionAccountRole, Pubkey), ExecutionAccountEvidenceRecord>,
    by_base_mint_role: DashMap<(Pubkey, ExecutionAccountRole), Vec<Pubkey>>,
    by_pool_role: DashMap<(Pubkey, ExecutionAccountRole), Vec<Pubkey>>,
}

impl ExecutionAccountEvidenceStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(
        &self,
        evidence: ExecutionAccountEvidence,
    ) -> UpsertExecutionAccountEvidenceResult {
        if let Some(base_mint) = evidence.base_mint {
            index_pubkey(
                &self.by_base_mint_role,
                base_mint,
                evidence.role.clone(),
                evidence.account_pubkey,
            );
        }
        if let Some(pool_id) = evidence.pool_id {
            index_pubkey(
                &self.by_pool_role,
                pool_id,
                evidence.role.clone(),
                evidence.account_pubkey,
            );
        }

        let (role, account_pubkey) = evidence.exact_key();
        match self.by_role_pubkey.entry((role.clone(), account_pubkey)) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().merge(evidence);
                UpsertExecutionAccountEvidenceResult::from_record(
                    UpsertExecutionAccountEvidenceOutcome::Updated,
                    role,
                    account_pubkey,
                    entry.get(),
                )
            }
            Entry::Vacant(entry) => {
                let record = ExecutionAccountEvidenceRecord::new(evidence);
                let inserted = entry.insert(record);
                UpsertExecutionAccountEvidenceResult::from_record(
                    UpsertExecutionAccountEvidenceOutcome::Inserted,
                    role,
                    account_pubkey,
                    &inserted,
                )
            }
        }
    }

    #[must_use]
    pub fn get(
        &self,
        role: ExecutionAccountRole,
        account_pubkey: &Pubkey,
    ) -> Option<ExecutionAccountEvidenceRecord> {
        self.by_role_pubkey
            .get(&(role, *account_pubkey))
            .map(|entry| entry.clone())
    }

    #[must_use]
    pub fn find_by_base_mint_role(
        &self,
        base_mint: &Pubkey,
        role: ExecutionAccountRole,
    ) -> Vec<Pubkey> {
        self.by_base_mint_role
            .get(&(*base_mint, role))
            .map(|entry| entry.clone())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn find_by_pool_role(&self, pool_id: &Pubkey, role: ExecutionAccountRole) -> Vec<Pubkey> {
        self.by_pool_role
            .get(&(*pool_id, role))
            .map(|entry| entry.clone())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn snapshot_counts(&self) -> ExecutionAccountEvidenceSnapshotCounts {
        let mut counts = ExecutionAccountEvidenceSnapshotCounts::default();
        counts.total_records = self.by_role_pubkey.len();

        for entry in self.by_role_pubkey.iter() {
            let record = entry.value();
            if record.best_positive.is_some() {
                counts.positive_records += 1;
            }
            if record.latest_negative.is_some() {
                counts.negative_records += 1;
            }
            if record.conflict.is_some() {
                counts.conflict_records += 1;
            }
            if record
                .best_positive
                .as_ref()
                .map(|evidence| evidence.evidence_ready)
                .unwrap_or(false)
            {
                counts.evidence_ready_records += 1;
            }
            *counts
                .latest_status_counts
                .entry(record.latest.status.as_str().to_string())
                .or_insert(0) += 1;
            *counts
                .latest_source_counts
                .entry(record.latest.source.as_str().to_string())
                .or_insert(0) += 1;
        }

        counts
    }
}

fn should_replace_best_positive(
    candidate: &ExecutionAccountEvidence,
    current: &ExecutionAccountEvidence,
) -> bool {
    let candidate_precedence = candidate.status.precedence();
    let current_precedence = current.status.precedence();
    candidate_precedence > current_precedence
        || (candidate_precedence == current_precedence
            && candidate.received_at_ms >= current.received_at_ms)
}

fn index_pubkey(
    index: &DashMap<(Pubkey, ExecutionAccountRole), Vec<Pubkey>>,
    identity: Pubkey,
    role: ExecutionAccountRole,
    account_pubkey: Pubkey,
) {
    match index.entry((identity, role)) {
        Entry::Occupied(mut entry) => {
            let pubkeys = entry.get_mut();
            if !pubkeys.contains(&account_pubkey) {
                pubkeys.push(account_pubkey);
            }
        }
        Entry::Vacant(entry) => {
            entry.insert(vec![account_pubkey]);
        }
    }
}
