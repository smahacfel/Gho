//! Standalone prospective Exact-State Pump Research Tape V2 operator binary.
//!
//! It cannot open the historical GO-D run, run a strategy, export outcomes,
//! call GO-E, or modify active Ghost runtime behavior.  `capture` is an
//! explicitly separate future source operation; `preflight` is local-only.

use anyhow::{bail, Result};
use seer::research_exact_tape_v2::{
    create_operator_preflight_v2_from_config_path,
    run_prospective_exact_state_capture_v2_from_config_path,
};
use std::{collections::BTreeMap, env, ffi::OsString, path::PathBuf};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pump_exact_state_tape_v2=info,seer=info,warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut arguments = env::args_os();
    let executable = arguments
        .next()
        .unwrap_or_else(|| "pump-exact-state-tape-v2".into());
    let Some(command) = arguments.next() else {
        print_usage(&executable);
        bail!("missing prospective Exact-State Tape V2 command");
    };
    let command = command
        .into_string()
        .map_err(|_| anyhow::anyhow!("V2 command is not valid UTF-8"))?;
    let arguments: Vec<_> = arguments.collect();
    if is_help(&arguments) || matches!(command.as_str(), "help" | "--help" | "-h") {
        print_usage(&executable);
        return Ok(());
    }
    match command.as_str() {
        "preflight" => {
            let paths = parse_named_paths(arguments, "preflight", &["--config", "--output"])?;
            let config_path = &paths[0];
            let output_dir = &paths[1];
            let summary = create_operator_preflight_v2_from_config_path(config_path, output_dir)?;
            info!(
                bundle_dir = %summary.bundle_dir.display(),
                receipt_path = %summary.receipt_path.display(),
                receipt_sha256 = %summary.receipt_digest.sha256,
                sealed_release_binary_sha256 = %summary.sealed_release_binary_digest.sha256,
                "prospective Exact-State Tape V2 operator preflight created a fresh sealed release bundle; no provider I/O was performed"
            );
            Ok(())
        }
        "capture" => {
            let paths =
                parse_named_paths(arguments, "capture", &["--config", "--preflight-receipt"])?;
            let config_path = &paths[0];
            let preflight_receipt_path = &paths[1];
            let summary = run_prospective_exact_state_capture_v2_from_config_path(
                config_path,
                preflight_receipt_path,
            )
            .await?;
            if !summary.is_complete() {
                error!(
                    run_id = %summary.run_id,
                    raw_dir = %summary.raw_dir.display(),
                    gap_count = summary.gap_count,
                    source_error = ?summary.source_error,
                    writer_error = ?summary.writer_error,
                    completion_receipt_error = ?summary.completion_receipt_error,
                    "prospective Exact-State Tape V2 retained an incomplete raw run; it is not eligible for qualification"
                );
                bail!(
                    "prospective Exact-State Tape V2 capture is incomplete; inspect {}/run_completion_receipt_v2.json",
                    summary.raw_dir.display()
                );
            }
            info!(
                run_id = %summary.run_id,
                raw_dir = %summary.raw_dir.display(),
                "prospective Exact-State Tape V2 raw capture completed; exact-state qualification remains a separate offline gate"
            );
            Ok(())
        }
        _ => {
            print_usage(&executable);
            bail!("unknown prospective Exact-State Tape V2 command {command:?}")
        }
    }
}

fn is_help(arguments: &[OsString]) -> bool {
    matches!(arguments, [argument] if argument == "--help" || argument == "-h")
}

fn parse_named_paths(
    arguments: Vec<OsString>,
    command: &str,
    required_flags: &[&str],
) -> Result<Vec<PathBuf>> {
    if arguments.len() != required_flags.len().saturating_mul(2) {
        bail!(
            "{command} requires exactly {}",
            required_flags
                .iter()
                .map(|flag| format!("{flag} <path>"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    let mut iterator = arguments.into_iter();
    let mut values = BTreeMap::<String, PathBuf>::new();
    while let Some(flag) = iterator.next() {
        let flag = flag
            .into_string()
            .map_err(|_| anyhow::anyhow!("{command} argument flag is not valid UTF-8"))?;
        if !required_flags.contains(&flag.as_str()) {
            bail!("{command} does not accept argument {flag:?}");
        }
        let Some(path) = iterator.next() else {
            bail!("{command} requires a path after {flag}");
        };
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty() {
            bail!("{command} path after {flag} must not be empty");
        }
        if values.insert(flag.clone(), path).is_some() {
            bail!("{command} received duplicate argument {flag}");
        }
    }
    required_flags
        .iter()
        .map(|flag| {
            values
                .remove(*flag)
                .ok_or_else(|| anyhow::anyhow!("{command} requires {flag} <path>"))
        })
        .collect()
}

fn print_usage(executable: &OsString) {
    eprintln!(
        "Usage:\n  {} preflight --config /protected/operator/pump-exact-state-tape-v2.toml --output /protected/operator/pump-exact-state-v2-preflight-<id>\n  /protected/operator/pump-exact-state-v2-preflight-<id>/release/pump-exact-state-tape-v2 capture --config /protected/operator/pump-exact-state-tape-v2.toml --preflight-receipt /protected/operator/pump-exact-state-v2-preflight-<id>/operator_preflight_receipt_v2.json\n\n`preflight` is local-only: it requires a clean repository, a release bootstrap binary, private isolated V2 output root, and enough filesystem capacity for max_raw_bytes + min_free_bytes + V2 metadata allowance. It builds the V2 binary anew with Cargo locked/offline into an isolated target, retains the build log and receipt, then copies the resulting release executable into a sealed bundle. The later `capture` command must be launched from that copied bundle executable; it rejects a mismatched process image before any provider I/O. After this local authority gate, `capture` opens the all-Pump-owned Yellowstone source stream plus unfiltered full-block evidence lane, persists a bounded finalized source-provider bootstrap snapshot, and writes one new raw-v2 run. The cohort ends only by SIGINT or the hash-pinned wall deadline; source gaps, reconnects, queue/byte-budget breaches, or storage-floor breaches fail closed. It never reads or repairs GO-D, invokes GO-E, exports outcomes, or changes the active Ghost runtime. A raw-complete V2 run is not itself an ExactStateCapability=Qualified result; qualification and strategy use remain separate offline gates.",
        executable.to_string_lossy()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_requires_each_named_v2_authority_path_once() {
        assert_eq!(
            parse_named_paths(
                vec![
                    "--output".into(),
                    "preflight".into(),
                    "--config".into(),
                    "v2.toml".into(),
                ],
                "preflight",
                &["--config", "--output"],
            )
            .expect("valid V2 preflight paths"),
            vec![PathBuf::from("v2.toml"), PathBuf::from("preflight")]
        );
        assert!(parse_named_paths(Vec::new(), "preflight", &["--config", "--output"]).is_err());
        assert!(parse_named_paths(
            vec!["--other".into(), "v2.toml".into()],
            "preflight",
            &["--config", "--output"],
        )
        .is_err());
        assert!(parse_named_paths(
            vec![
                "--config".into(),
                "one.toml".into(),
                "--config".into(),
                "two.toml".into(),
            ],
            "capture",
            &["--config", "--preflight-receipt"],
        )
        .is_err());
    }
}
