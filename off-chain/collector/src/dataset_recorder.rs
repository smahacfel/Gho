use crate::error::DatasetError;
use crate::types::DatasetManifest;
use chrono::Utc;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::fs;
use tokio::io::AsyncWriteExt;

pub const DEFAULT_DATASET_DIR: &str = "./datasets/dry_run";

pub struct DatasetRecorder {
    base_dir: PathBuf,
}

impl DatasetRecorder {
    pub fn new() -> Self {
        Self {
            base_dir: PathBuf::from(DEFAULT_DATASET_DIR),
        }
    }

    pub fn with_base_dir(path: impl AsRef<Path>) -> Self {
        Self {
            base_dir: path.as_ref().to_path_buf(),
        }
    }

    pub async fn record_dataset<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10>(
        &self,
        slot: u64,
        tx_sig: &str,
        raw_tx: &T1,
        seer_event: &T2,
        ssmi: Option<&T3>,
        mpcf: Option<&T4>,
        iwim: Option<&T5>,
        scr: Option<&T6>,
        ulvf: Option<&T7>,
        povc: Option<&T8>,
        qass: Option<&T9>,
        hyper: &T10,
    ) -> Result<PathBuf, DatasetError>
    where
        T1: Serialize,
        T2: Serialize,
        T3: Serialize,
        T4: Serialize,
        T5: Serialize,
        T6: Serialize,
        T7: Serialize,
        T8: Serialize,
        T9: Serialize,
        T10: Serialize,
    {
        let start = Instant::now();

        if tx_sig.len() < 32 {
            return Err(DatasetError::InvalidSignature(tx_sig.to_string()));
        }

        let sig_short = &tx_sig[..16];
        let dir_name = format!("{}_{}", slot, sig_short);
        let dataset_dir = self.base_dir.join(&dir_name);
        fs::create_dir_all(&dataset_dir).await?;

        let mut manifest = DatasetManifest {
            slot,
            tx_signature: tx_sig.to_string(),
            recorded_at: Utc::now(),
            files_written: Vec::new(),
            missing_components: Vec::new(),
            recording_duration_ms: 0,
        };

        // Write required files
        self.write_json(&dataset_dir, "raw_tx.json", raw_tx).await?;
        manifest.files_written.push("raw_tx.json".into());

        self.write_json(&dataset_dir, "seer_event.json", seer_event)
            .await?;
        manifest.files_written.push("seer_event.json".into());

        self.write_json(&dataset_dir, "hyper_prediction.json", hyper)
            .await?;
        manifest.files_written.push("hyper_prediction.json".into());

        // Write optional files
        self.write_optional(&dataset_dir, "ssmi.json", ssmi, &mut manifest, "ssmi")
            .await?;
        self.write_optional(&dataset_dir, "mpcf.json", mpcf, &mut manifest, "mpcf")
            .await?;
        self.write_optional(&dataset_dir, "iwim.json", iwim, &mut manifest, "iwim")
            .await?;
        self.write_optional(&dataset_dir, "scr.json", scr, &mut manifest, "scr")
            .await?;
        self.write_optional(&dataset_dir, "ulvf.json", ulvf, &mut manifest, "ulvf")
            .await?;
        self.write_optional(&dataset_dir, "povc.json", povc, &mut manifest, "povc")
            .await?;
        self.write_optional(&dataset_dir, "qass.json", qass, &mut manifest, "qass")
            .await?;

        manifest.recording_duration_ms = start.elapsed().as_millis() as u64;
        self.write_json(&dataset_dir, "manifest.json", &manifest)
            .await?;

        Ok(dataset_dir)
    }

    async fn write_json<T: Serialize>(
        &self,
        dir: &Path,
        name: &str,
        data: &T,
    ) -> Result<(), DatasetError> {
        let path = dir.join(name);
        let json = serde_json::to_string_pretty(data)?;
        let mut file = fs::File::create(&path).await?;
        file.write_all(json.as_bytes()).await?;
        Ok(())
    }

    async fn write_optional<T: Serialize>(
        &self,
        dir: &Path,
        name: &str,
        data: Option<&T>,
        manifest: &mut DatasetManifest,
        component: &str,
    ) -> Result<(), DatasetError> {
        match data {
            Some(d) => {
                self.write_json(dir, name, d).await?;
                manifest.files_written.push(name.into());
            }
            None => manifest.missing_components.push(component.into()),
        }
        Ok(())
    }
}

impl Default for DatasetRecorder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn record_dataset_async<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10>(
    slot: u64,
    tx_sig: String,
    raw_tx: T1,
    seer_event: T2,
    ssmi: Option<T3>,
    mpcf: Option<T4>,
    iwim: Option<T5>,
    scr: Option<T6>,
    ulvf: Option<T7>,
    povc: Option<T8>,
    qass: Option<T9>,
    hyper: T10,
) where
    T1: Serialize + Send + Sync + 'static,
    T2: Serialize + Send + Sync + 'static,
    T3: Serialize + Send + Sync + 'static,
    T4: Serialize + Send + Sync + 'static,
    T5: Serialize + Send + Sync + 'static,
    T6: Serialize + Send + Sync + 'static,
    T7: Serialize + Send + Sync + 'static,
    T8: Serialize + Send + Sync + 'static,
    T9: Serialize + Send + Sync + 'static,
    T10: Serialize + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let recorder = DatasetRecorder::new();
        let _ = recorder
            .record_dataset(
                slot,
                &tx_sig,
                &raw_tx,
                &seer_event,
                ssmi.as_ref(),
                mpcf.as_ref(),
                iwim.as_ref(),
                scr.as_ref(),
                ulvf.as_ref(),
                povc.as_ref(),
                qass.as_ref(),
                &hyper,
            )
            .await;
    });
}
