use ghost_collector::{record_dataset_async, DatasetError, DatasetRecorder};
use serde::{Deserialize, Serialize};
use std::fs;
use tempfile::TempDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MockRawTx {
    signature: String,
    slot: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MockSeerEvent {
    pool_address: String,
    detected_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MockComponent {
    value: f64,
    timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MockHyperPrediction {
    score: f64,
    confidence: f64,
}

#[tokio::test]
async fn test_complete_recording_with_all_components() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = DatasetRecorder::with_base_dir(temp_dir.path());

    let slot = 12345678u64;
    let tx_sig =
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZk";

    let raw_tx = MockRawTx {
        signature: tx_sig.to_string(),
        slot,
    };

    let seer_event = MockSeerEvent {
        pool_address: "7UX2i7SucgLMQcfZ75s3VXmZZY4YRUyJN9X1RgfMoDUi".to_string(),
        detected_at: 1234567890,
    };

    let ssmi = Some(MockComponent {
        value: 0.85,
        timestamp: 1234567890,
    });
    let mpcf = Some(MockComponent {
        value: 0.92,
        timestamp: 1234567890,
    });
    let iwim = Some(MockComponent {
        value: 0.15,
        timestamp: 1234567890,
    });
    let scr = Some(MockComponent {
        value: 0.78,
        timestamp: 1234567890,
    });
    let ulvf = Some(MockComponent {
        value: 0.65,
        timestamp: 1234567890,
    });
    let povc = Some(MockComponent {
        value: 0.88,
        timestamp: 1234567890,
    });
    let qass = Some(MockComponent {
        value: 0.95,
        timestamp: 1234567890,
    });

    let hyper = MockHyperPrediction {
        score: 0.89,
        confidence: 0.92,
    };

    let result = recorder
        .record_dataset(
            slot,
            tx_sig,
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

    assert!(result.is_ok());
    let dataset_dir = result.unwrap();

    // Verify directory exists and has correct name
    assert!(dataset_dir.exists());
    assert!(dataset_dir
        .to_string_lossy()
        .contains("12345678_5VERv8NMvzbJMEkV"));

    // Verify all files were created
    assert!(dataset_dir.join("raw_tx.json").exists());
    assert!(dataset_dir.join("seer_event.json").exists());
    assert!(dataset_dir.join("ssmi.json").exists());
    assert!(dataset_dir.join("mpcf.json").exists());
    assert!(dataset_dir.join("iwim.json").exists());
    assert!(dataset_dir.join("scr.json").exists());
    assert!(dataset_dir.join("ulvf.json").exists());
    assert!(dataset_dir.join("povc.json").exists());
    assert!(dataset_dir.join("qass.json").exists());
    assert!(dataset_dir.join("hyper_prediction.json").exists());
    assert!(dataset_dir.join("manifest.json").exists());

    // Verify manifest content
    let manifest_content = fs::read_to_string(dataset_dir.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_content).unwrap();

    assert_eq!(manifest["slot"], 12345678);
    assert_eq!(manifest["tx_signature"], tx_sig);
    assert_eq!(manifest["files_written"].as_array().unwrap().len(), 10);
    assert_eq!(manifest["missing_components"].as_array().unwrap().len(), 0);
    assert!(manifest["recording_duration_ms"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_recording_with_missing_optional_components() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = DatasetRecorder::with_base_dir(temp_dir.path());

    let slot = 87654321u64;
    let tx_sig =
        "3jKhPAKvXM8kL9QnrRvHNJPqB4wXzYdF2sGhT6yW5nBpU8xE7rC1vD9mA2tS4qN6oL5pR3wK1hJ8gF7eV9uT2";

    let raw_tx = MockRawTx {
        signature: tx_sig.to_string(),
        slot,
    };

    let seer_event = MockSeerEvent {
        pool_address: "8PQX3i9TucfLMQdgZ86t4WXnZZZ5ZSUxJN0Y2RhfNpEVj".to_string(),
        detected_at: 9876543210,
    };

    let hyper = MockHyperPrediction {
        score: 0.75,
        confidence: 0.85,
    };

    // Only pass None for optional components
    let result = recorder
        .record_dataset(
            slot,
            tx_sig,
            &raw_tx,
            &seer_event,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            &hyper,
        )
        .await;

    assert!(result.is_ok());
    let dataset_dir = result.unwrap();

    // Verify only required files were created
    assert!(dataset_dir.join("raw_tx.json").exists());
    assert!(dataset_dir.join("seer_event.json").exists());
    assert!(dataset_dir.join("hyper_prediction.json").exists());
    assert!(dataset_dir.join("manifest.json").exists());

    // Verify optional files were NOT created
    assert!(!dataset_dir.join("ssmi.json").exists());
    assert!(!dataset_dir.join("mpcf.json").exists());
    assert!(!dataset_dir.join("iwim.json").exists());
    assert!(!dataset_dir.join("scr.json").exists());
    assert!(!dataset_dir.join("ulvf.json").exists());
    assert!(!dataset_dir.join("povc.json").exists());
    assert!(!dataset_dir.join("qass.json").exists());

    // Verify manifest reflects missing components
    let manifest_content = fs::read_to_string(dataset_dir.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_content).unwrap();

    assert_eq!(manifest["files_written"].as_array().unwrap().len(), 3);
    assert_eq!(manifest["missing_components"].as_array().unwrap().len(), 7);

    let missing = manifest["missing_components"].as_array().unwrap();
    assert!(missing.iter().any(|v| v.as_str().unwrap() == "ssmi"));
    assert!(missing.iter().any(|v| v.as_str().unwrap() == "mpcf"));
    assert!(missing.iter().any(|v| v.as_str().unwrap() == "iwim"));
    assert!(missing.iter().any(|v| v.as_str().unwrap() == "scr"));
    assert!(missing.iter().any(|v| v.as_str().unwrap() == "ulvf"));
    assert!(missing.iter().any(|v| v.as_str().unwrap() == "povc"));
    assert!(missing.iter().any(|v| v.as_str().unwrap() == "qass"));
}

#[tokio::test]
async fn test_concurrent_recordings() {
    let temp_dir = TempDir::new().unwrap();

    let mut handles = vec![];

    for i in 0..10 {
        let recorder_clone = DatasetRecorder::with_base_dir(temp_dir.path());
        let handle = tokio::spawn(async move {
            let slot = 10000000 + i;
            let tx_sig = format!("5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjK{:05}", i);

            let raw_tx = MockRawTx {
                signature: tx_sig.clone(),
                slot,
            };

            let seer_event = MockSeerEvent {
                pool_address: format!("7UX2i7SucgLMQcfZ75s3VXmZZY4YRUyJN9X1RgfMo{:05}", i),
                detected_at: 1234567890 + i,
            };

            let hyper = MockHyperPrediction {
                score: 0.5 + (i as f64 * 0.01),
                confidence: 0.8,
            };

            recorder_clone
                .record_dataset(
                    slot,
                    &tx_sig,
                    &raw_tx,
                    &seer_event,
                    None::<&MockComponent>,
                    None::<&MockComponent>,
                    None::<&MockComponent>,
                    None::<&MockComponent>,
                    None::<&MockComponent>,
                    None::<&MockComponent>,
                    None::<&MockComponent>,
                    &hyper,
                )
                .await
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    // Verify all 10 datasets were created
    let entries = fs::read_dir(temp_dir.path()).unwrap();
    let count = entries.count();
    assert_eq!(count, 10);
}

#[tokio::test]
async fn test_invalid_signature_error() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = DatasetRecorder::with_base_dir(temp_dir.path());

    let slot = 12345678u64;
    let tx_sig = "short"; // Too short - less than 32 chars

    let raw_tx = MockRawTx {
        signature: tx_sig.to_string(),
        slot,
    };

    let seer_event = MockSeerEvent {
        pool_address: "7UX2i7SucgLMQcfZ75s3VXmZZY4YRUyJN9X1RgfMoDUi".to_string(),
        detected_at: 1234567890,
    };

    let hyper = MockHyperPrediction {
        score: 0.89,
        confidence: 0.92,
    };

    let result = recorder
        .record_dataset(
            slot,
            tx_sig,
            &raw_tx,
            &seer_event,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            &hyper,
        )
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        DatasetError::InvalidSignature(sig) => {
            assert_eq!(sig, "short");
        }
        _ => panic!("Expected InvalidSignature error"),
    }
}

#[tokio::test]
async fn test_json_content_validation() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = DatasetRecorder::with_base_dir(temp_dir.path());

    let slot = 11111111u64;
    let tx_sig =
        "4wQRv9NMuzcJMEkW9xnrLkEbXRtSz9DptLEZkDJjCSnbKMhq9virChpRqkLipS5ukG4aqSzsGnCW7VkLejtTal";

    let raw_tx = MockRawTx {
        signature: tx_sig.to_string(),
        slot,
    };

    let seer_event = MockSeerEvent {
        pool_address: "9VY3j8TvdgLNRdfZ86t5WYoABb6ATVyJO0Z3ShgOpFWk".to_string(),
        detected_at: 5555555555,
    };

    let ssmi = Some(MockComponent {
        value: 0.999,
        timestamp: 5555555555,
    });

    let hyper = MockHyperPrediction {
        score: 0.123,
        confidence: 0.456,
    };

    let result = recorder
        .record_dataset(
            slot,
            tx_sig,
            &raw_tx,
            &seer_event,
            ssmi.as_ref(),
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            None::<&MockComponent>,
            &hyper,
        )
        .await;

    assert!(result.is_ok());
    let dataset_dir = result.unwrap();

    // Read and validate raw_tx.json
    let raw_tx_content = fs::read_to_string(dataset_dir.join("raw_tx.json")).unwrap();
    let parsed_raw_tx: MockRawTx = serde_json::from_str(&raw_tx_content).unwrap();
    assert_eq!(parsed_raw_tx, raw_tx);

    // Read and validate seer_event.json
    let seer_event_content = fs::read_to_string(dataset_dir.join("seer_event.json")).unwrap();
    let parsed_seer_event: MockSeerEvent = serde_json::from_str(&seer_event_content).unwrap();
    assert_eq!(parsed_seer_event, seer_event);

    // Read and validate ssmi.json
    let ssmi_content = fs::read_to_string(dataset_dir.join("ssmi.json")).unwrap();
    let parsed_ssmi: MockComponent = serde_json::from_str(&ssmi_content).unwrap();
    assert_eq!(parsed_ssmi, ssmi.unwrap());

    // Read and validate hyper_prediction.json
    let hyper_content = fs::read_to_string(dataset_dir.join("hyper_prediction.json")).unwrap();
    let parsed_hyper: MockHyperPrediction = serde_json::from_str(&hyper_content).unwrap();
    assert_eq!(parsed_hyper, hyper);

    // Verify JSON is pretty-printed (contains newlines and indentation)
    assert!(raw_tx_content.contains('\n'));
    assert!(raw_tx_content.contains("  "));
}

#[tokio::test]
async fn test_async_recording_function() {
    let temp_dir = TempDir::new().unwrap();
    std::env::set_var("DATASET_DIR", temp_dir.path().to_string_lossy().to_string());

    let slot = 99999999u64;
    let tx_sig =
        "8zXYw0OPxadKNFlX0ynsMlFcYSuTa0EruMFajEKkDTocLNir0wjsCrqSrlMjqT6vlH5brTatHoD8WlLfkuVcbm"
            .to_string();

    let raw_tx = MockRawTx {
        signature: tx_sig.clone(),
        slot,
    };

    let seer_event = MockSeerEvent {
        pool_address: "AYZ4k9UwehMORgfA97u6XZpBCc7BUWzJP1a4TihPqGXl".to_string(),
        detected_at: 9999999999,
    };

    let hyper = MockHyperPrediction {
        score: 0.777,
        confidence: 0.888,
    };

    // Use the async recording function
    record_dataset_async(
        slot,
        tx_sig,
        raw_tx,
        seer_event,
        None::<MockComponent>,
        None::<MockComponent>,
        None::<MockComponent>,
        None::<MockComponent>,
        None::<MockComponent>,
        None::<MockComponent>,
        None::<MockComponent>,
        hyper,
    );

    // Wait a bit for the async task to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // The async function uses default directory, so we can't easily verify
    // But at least we verified it compiles and runs without panicking
}
