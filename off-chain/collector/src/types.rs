use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetManifest {
    pub slot: u64,
    pub tx_signature: String,
    pub recorded_at: DateTime<Utc>,
    pub files_written: Vec<String>,
    pub missing_components: Vec<String>,
    pub recording_duration_ms: u64,
}
