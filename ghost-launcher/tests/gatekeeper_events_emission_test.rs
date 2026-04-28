//! Test: Gatekeeper decision events are emitted to events JSONL.
//!
//! Verifies that after a REJECT or TIMEOUT verdict, at least 1 event
//! is written to the events stream (CandidateFinalized via emit_candidate).
//!
//! Run with: cargo test -p ghost-launcher gatekeeper_events_emission -- --nocapture

use ghost_brain::events::{EventEmitter, EventWriterConfig};
use ghost_brain::execution::backend::Lane;
use tempfile::TempDir;

/// Helper: create an EventEmitter writing to a temp directory.
fn make_test_emitter() -> (EventEmitter, TempDir) {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let config = EventWriterConfig {
        output_dir: tmp.path().to_str().unwrap().to_string(),
        enable_optional_events: true,
        ..Default::default()
    };
    let emitter = EventEmitter::new(config, "test-launcher-run".to_string(), Lane::Paper)
        .expect("Failed to create EventEmitter for test");
    (emitter, tmp)
}

#[test]
fn test_reject_verdict_emits_event() {
    let (emitter, _tmp) = make_test_emitter();

    // Simulate the same call that oracle_runtime makes on REJECT
    emitter.emit_candidate(
        &"PoolABC123reject".to_string(),
        None,
        None,
        "REJECT",
        vec!["low_cv".to_string(), "cabal".to_string()],
        "gatekeeper_v2",
    );
    emitter.flush().unwrap();

    assert!(
        emitter.total_events_written() > 0,
        "Expected at least 1 event after REJECT verdict, got 0"
    );
}

#[test]
fn test_timeout_verdict_emits_event() {
    let (emitter, _tmp) = make_test_emitter();

    // Simulate the same call that oracle_runtime makes on TIMEOUT
    emitter.emit_candidate(
        &"PoolXYZ789timeout".to_string(),
        None,
        None,
        "TIMEOUT",
        vec![],
        "gatekeeper_v2",
    );
    emitter.flush().unwrap();

    assert!(
        emitter.total_events_written() > 0,
        "Expected at least 1 event after TIMEOUT verdict, got 0"
    );
}

#[test]
fn test_pass_verdict_emits_event() {
    let (emitter, _tmp) = make_test_emitter();

    // Simulate the same call that oracle_runtime makes on PASS/BUY
    emitter.emit_candidate(
        &"PoolDEF456pass".to_string(),
        None,
        None,
        "PASS",
        vec![],
        "gatekeeper_v2",
    );
    emitter.flush().unwrap();

    assert!(
        emitter.total_events_written() > 0,
        "Expected at least 1 event after PASS verdict, got 0"
    );
}

#[test]
fn test_events_written_to_jsonl_file() {
    let (emitter, tmp) = make_test_emitter();

    // Emit events for all verdict types
    emitter.emit_candidate(
        &"pool-reject".to_string(),
        None,
        None,
        "REJECT",
        vec!["low_entropy".to_string()],
        "gatekeeper_v2",
    );
    emitter.emit_candidate(
        &"pool-timeout".to_string(),
        None,
        None,
        "TIMEOUT",
        vec![],
        "gatekeeper_v2",
    );
    emitter.emit_candidate(
        &"pool-pass".to_string(),
        None,
        None,
        "PASS",
        vec![],
        "gatekeeper_v2",
    );
    emitter.flush().unwrap();

    assert_eq!(emitter.total_events_written(), 3);

    // Verify JSONL file exists and has content
    let mut found_files = 0;
    let mut total_lines = 0;
    for entry in std::fs::read_dir(tmp.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().map_or(false, |ext| ext == "jsonl") {
            found_files += 1;
            let content = std::fs::read_to_string(entry.path()).unwrap();
            for line in content.lines() {
                total_lines += 1;
                // Verify each line is valid JSON containing expected fields
                let parsed: serde_json::Value =
                    serde_json::from_str(line).expect("Each JSONL line must be valid JSON");
                assert!(
                    parsed.get("kind").is_some(),
                    "Event must have a 'kind' field"
                );
            }
        }
    }
    assert!(
        found_files > 0,
        "Expected at least 1 JSONL file in output dir"
    );
    assert_eq!(total_lines, 3, "Expected 3 event lines in JSONL file(s)");
}
