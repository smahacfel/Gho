use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Seek, Write};
use std::path::{Path, PathBuf};

use crate::aem::{
    error::AemError,
    types::{
        AemLedgerReader, AemLedgerWriter, ManagementDecisionEvent, ManagementOutcomeEvent,
        RegimeIndexRecord, ReplayPair, TimeIndexRecord, UnixMs,
    },
};

const DECISIONS_FILE: &str = "aem_decisions.jsonl";
const OUTCOMES_FILE: &str = "aem_outcomes.jsonl";
const IDX_TIME_FILE: &str = "aem_idx_time.jsonl";
const IDX_REGIME_FILE: &str = "aem_idx_regime.jsonl";

#[derive(Debug, Clone)]
pub struct JsonlAemLedger {
    base_dir: PathBuf,
}

impl JsonlAemLedger {
    pub fn new(base_dir: impl Into<PathBuf>) -> Result<Self, AemError> {
        let base_dir = base_dir.into();
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    fn decisions_path(&self) -> PathBuf {
        self.base_dir.join(DECISIONS_FILE)
    }

    fn outcomes_path(&self) -> PathBuf {
        self.base_dir.join(OUTCOMES_FILE)
    }

    fn idx_time_path(&self) -> PathBuf {
        self.base_dir.join(IDX_TIME_FILE)
    }

    fn idx_regime_path(&self) -> PathBuf {
        self.base_dir.join(IDX_REGIME_FILE)
    }
}

fn append_jsonl<T: serde::Serialize>(path: &Path, value: &T) -> Result<u64, AemError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)?;
    let offset = file.seek(std::io::SeekFrom::End(0))?;
    let line = serde_json::to_string(value)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(offset)
}

fn read_jsonl<T: for<'de> serde::Deserialize<'de>>(path: &Path) -> Result<Vec<T>, AemError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = OpenOptions::new().read(true).open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines() {
        let raw = line?;
        if raw.trim().is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<T>(&raw)?;
        out.push(parsed);
    }
    Ok(out)
}

impl AemLedgerWriter for JsonlAemLedger {
    fn append_decision(&self, event: &ManagementDecisionEvent) -> Result<(), AemError> {
        let offset = append_jsonl(&self.decisions_path(), event)?;
        self.append_time_index(&TimeIndexRecord {
            timestamp_unix_ms: event.timestamp_decision_unix_ms,
            event_type: "decision".to_string(),
            event_id: event.decision_event_id.clone(),
            file_offset: offset,
        })?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        event.regime_key.hash(&mut hasher);
        let regime_key_hash = hasher.finish();
        self.append_regime_index(&RegimeIndexRecord {
            regime_key_hash,
            action: event.action_chosen,
            timestamp_unix_ms: event.timestamp_decision_unix_ms,
            decision_event_id: event.decision_event_id.clone(),
            file_offset: offset,
        })?;
        Ok(())
    }

    fn append_outcome(&self, event: &ManagementOutcomeEvent) -> Result<(), AemError> {
        let offset = append_jsonl(&self.outcomes_path(), event)?;
        self.append_time_index(&TimeIndexRecord {
            timestamp_unix_ms: event.timestamp_outcome_unix_ms,
            event_type: "outcome".to_string(),
            event_id: event.outcome_event_id.clone(),
            file_offset: offset,
        })?;
        Ok(())
    }

    fn append_time_index(&self, idx: &TimeIndexRecord) -> Result<(), AemError> {
        let _ = append_jsonl(&self.idx_time_path(), idx)?;
        Ok(())
    }

    fn append_regime_index(&self, idx: &RegimeIndexRecord) -> Result<(), AemError> {
        let _ = append_jsonl(&self.idx_regime_path(), idx)?;
        Ok(())
    }
}

impl AemLedgerReader for JsonlAemLedger {
    fn replay_pairs_in_window(
        &self,
        window_start_unix_ms: UnixMs,
        window_end_unix_ms: UnixMs,
        max_events: usize,
    ) -> Result<Vec<ReplayPair>, AemError> {
        let decisions: Vec<ManagementDecisionEvent> = read_jsonl(&self.decisions_path())?;
        let outcomes: Vec<ManagementOutcomeEvent> = read_jsonl(&self.outcomes_path())?;

        let mut decision_map = HashMap::new();
        for d in decisions.into_iter().filter(|d| {
            d.timestamp_decision_unix_ms >= window_start_unix_ms
                && d.timestamp_decision_unix_ms <= window_end_unix_ms
        }) {
            decision_map.insert(d.decision_event_id.clone(), d);
        }

        let mut pairs = Vec::new();
        for o in outcomes.into_iter().filter(|o| {
            o.timestamp_outcome_unix_ms >= window_start_unix_ms
                && o.timestamp_outcome_unix_ms <= window_end_unix_ms
        }) {
            if let Some(decision) = decision_map.get(&o.decision_event_id) {
                pairs.push(ReplayPair {
                    decision: decision.clone(),
                    outcome: o,
                });
            }
            if pairs.len() >= max_events {
                break;
            }
        }
        pairs.sort_by(|a, b| {
            a.outcome
                .timestamp_outcome_unix_ms
                .cmp(&b.outcome.timestamp_outcome_unix_ms)
        });
        Ok(pairs)
    }

    fn decisions_without_outcome(
        &self,
        window_start_unix_ms: UnixMs,
        window_end_unix_ms: UnixMs,
        max_events: usize,
    ) -> Result<Vec<ManagementDecisionEvent>, AemError> {
        let decisions: Vec<ManagementDecisionEvent> = read_jsonl(&self.decisions_path())?;
        let outcomes: Vec<ManagementOutcomeEvent> = read_jsonl(&self.outcomes_path())?;

        let mut with_outcome = HashSet::new();
        for o in outcomes {
            with_outcome.insert(o.decision_event_id);
        }

        let mut pending = Vec::new();
        for d in decisions.into_iter().filter(|d| {
            d.timestamp_decision_unix_ms >= window_start_unix_ms
                && d.timestamp_decision_unix_ms <= window_end_unix_ms
        }) {
            if !with_outcome.contains(&d.decision_event_id) {
                pending.push(d);
            }
            if pending.len() >= max_events {
                break;
            }
        }
        pending.sort_by(|a, b| {
            a.timestamp_decision_unix_ms
                .cmp(&b.timestamp_decision_unix_ms)
        });
        Ok(pending)
    }
}
