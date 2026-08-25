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
use seer::research_exact_tape_v2_materializer::{
    export_prospective_exact_state_outcome_blind_windows_v2,
    qualify_prospective_exact_state_raw_run_v2, validate_prospective_exact_state_strategy_input_v2,
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
        "qualify" => {
            let paths = parse_named_paths(
                arguments,
                "qualify",
                &["--raw-dir", "--semantics-manifest", "--output"],
            )?;
            let summary =
                qualify_prospective_exact_state_raw_run_v2(&paths[0], &paths[1], &paths[2])?;
            if !summary.is_qualified() {
                error!(
                    source_run_id = %summary.source_run_id,
                    output_dir = %summary.output_dir.display(),
                    receipt_path = %summary.receipt_path.display(),
                    blockers = ?summary.blockers,
                    exact_rooted_coverage_ppm = summary.exact_rooted_coverage_ppm,
                    exact_rooted_mutation_count = summary.exact_rooted_mutation_count,
                    successful_rooted_mutation_denominator = summary.successful_rooted_mutation_denominator,
                    "prospective Exact-State Tape V2 qualification published a blocked diagnostic artifact; strategy export remains forbidden"
                );
                bail!(
                    "prospective Exact-State Tape V2 is not qualified; inspect {}",
                    summary.receipt_path.display()
                );
            }
            info!(
                source_run_id = %summary.source_run_id,
                output_dir = %summary.output_dir.display(),
                receipt_path = %summary.receipt_path.display(),
                exact_rooted_coverage_ppm = summary.exact_rooted_coverage_ppm,
                exact_rooted_mutation_count = summary.exact_rooted_mutation_count,
                successful_rooted_mutation_denominator = summary.successful_rooted_mutation_denominator,
                "prospective Exact-State Tape V2 has passed its offline exact-state capability gate; strategy export remains a separate review gate"
            );
            Ok(())
        }
        "verify-strategy-input" => {
            let paths = parse_named_paths(
                arguments,
                "verify-strategy-input",
                &["--raw-dir", "--semantics-manifest", "--exact-dir"],
            )?;
            let authority = validate_prospective_exact_state_strategy_input_v2(
                &paths[0], &paths[1], &paths[2],
            )?;
            authority.revalidate_before_strategy_export_v2()?;
            info!(
                source_run_id = %authority.source_run_id,
                exact_dir = %authority.exact_dir.display(),
                exact_rooted_coverage_ppm = authority.exact_rooted_coverage_ppm,
                "prospective Exact-State Tape V2 exact artifact is a descriptor-pinned Qualified strategy-input authority; strategy export/outcomes remain separate review gates"
            );
            Ok(())
        }
        "export-window" => {
            let paths = parse_named_paths(
                arguments,
                "export-window",
                &[
                    "--raw-dir",
                    "--semantics-manifest",
                    "--exact-dir",
                    "--output",
                ],
            )?;
            let summary = export_prospective_exact_state_outcome_blind_windows_v2(
                &paths[0], &paths[1], &paths[2], &paths[3],
            )?;
            if !summary.has_complete_window() {
                error!(
                    source_run_id = %summary.source_run_id,
                    output_dir = %summary.output_dir.display(),
                    exported_birth_count = summary.exported_birth_count,
                    "prospective Exact-State Tape V2 published an outcome-blind window diagnostic with zero complete windows; strategy outcomes remain forbidden"
                );
                bail!(
                    "prospective Exact-State Tape V2 outcome-blind export has no complete windows; inspect {}",
                    summary.output_dir.display()
                );
            }
            info!(
                source_run_id = %summary.source_run_id,
                output_dir = %summary.output_dir.display(),
                exported_birth_count = summary.exported_birth_count,
                complete_window_count = summary.complete_window_count,
                time_axis = "observed_ingress_monotonic_ms",
                observation_ms = 150_000u64,
                forward_ms = 90_000u64,
                "prospective Exact-State Tape V2 created only outcome-blind windows; outcomes, strategy selection, Gatekeeper, and execution remain separate gates"
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
        "Usage:\n  {} preflight --config /protected/operator/pump-exact-state-tape-v2.toml --output /protected/operator/pump-exact-state-v2-preflight-<id>\n  /protected/operator/pump-exact-state-v2-preflight-<id>/release/pump-exact-state-tape-v2 capture --config /protected/operator/pump-exact-state-tape-v2.toml --preflight-receipt /protected/operator/pump-exact-state-v2-preflight-<id>/operator_preflight_receipt_v2.json\n  {} qualify --raw-dir /protected/research/raw-v2/<run-id> --semantics-manifest /protected/research/pump_exact_state_semantics_v2.json --output /protected/research/exact-v2/<run-id>\n  {} verify-strategy-input --raw-dir /protected/research/raw-v2/<run-id> --semantics-manifest /protected/research/pump_exact_state_semantics_v2.json --exact-dir /protected/research/exact-v2/<run-id>\n  {} export-window --raw-dir /protected/research/raw-v2/<run-id> --semantics-manifest /protected/research/pump_exact_state_semantics_v2.json --exact-dir /protected/research/exact-v2/<run-id> --output /protected/research/outcome-blind-v2/<run-id>\n\n`preflight` is local-only: it requires a clean repository, a release bootstrap binary, private isolated V2 output root, one hash-pinned Pump semantics manifest, and enough filesystem capacity for max_raw_bytes + min_free_bytes + V2 metadata allowance. It builds the V2 binary anew with Cargo locked/offline into an isolated target, retains the build log and receipt, then copies the resulting release executable into a sealed bundle. The later `capture` command must be launched from that copied bundle executable; it rejects a mismatched process image before any provider I/O. After this local authority gate, `capture` observes finalized ProgramData and refuses to allocate raw-v3 unless it matches the semantics manifest selected at preflight. It then opens the Pump-filtered transaction lane, canonical BondingCurve/Global account lanes, Slot, BlockMeta, and unfiltered full-block evidence lane. No account snapshot or Program-account scan is performed. Capture starts its cohort only after one retained and durably flushed five-lane stream-readiness boundary. The cohort ends only by SIGINT or the hash-pinned capture deadline; source gaps, reconnects, queue/byte-budget breaches, or storage-floor breaches fail closed. `qualify` is offline-only: it validates the complete V3 raw chain, stream-readiness boundary, full-block-versus-filtered-Pump reconciliation, pinned semantics, canonical streamed account anchors and occurrence conservation before writing an atomic exact-state artifact. A Blocked receipt is diagnostic only and returns non-zero. `verify-strategy-input` is read-only authority validation. `export-window` accepts only that same Qualified descriptor-pinned authority and emits fixed ingress-monotonic-time 150000ms observation / 90000ms forward-availability windows. It emits no outcome, strategy score, selection, Gatekeeper decision, or active-runtime change.",
        executable.to_string_lossy(),
        executable.to_string_lossy(),
        executable.to_string_lossy(),
        executable.to_string_lossy()
    );
    eprintln!(
        "V2 authority note: `qualify` requires a finalized parent-linked chain of per-slot BlockMeta/full-block identity pairs. `export-window` measures the 150000ms/90000ms gates on ingress-monotonic time and bounds forward availability only at that complete chain's tip after every required parent pair is reconciled; ingress wall time is audit-only."
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
        assert_eq!(
            parse_named_paths(
                vec![
                    "--output".into(),
                    "exact".into(),
                    "--raw-dir".into(),
                    "raw".into(),
                    "--semantics-manifest".into(),
                    "semantics.json".into(),
                ],
                "qualify",
                &["--raw-dir", "--semantics-manifest", "--output"],
            )
            .expect("valid V2 qualification paths"),
            vec![
                PathBuf::from("raw"),
                PathBuf::from("semantics.json"),
                PathBuf::from("exact"),
            ]
        );
        assert_eq!(
            parse_named_paths(
                vec![
                    "--output".into(),
                    "windows".into(),
                    "--exact-dir".into(),
                    "exact".into(),
                    "--semantics-manifest".into(),
                    "semantics.json".into(),
                    "--raw-dir".into(),
                    "raw".into(),
                ],
                "export-window",
                &[
                    "--raw-dir",
                    "--semantics-manifest",
                    "--exact-dir",
                    "--output"
                ],
            )
            .expect("valid V2 outcome-blind export paths"),
            vec![
                PathBuf::from("raw"),
                PathBuf::from("semantics.json"),
                PathBuf::from("exact"),
                PathBuf::from("windows"),
            ]
        );
        assert_eq!(
            parse_named_paths(
                vec![
                    "--exact-dir".into(),
                    "exact".into(),
                    "--semantics-manifest".into(),
                    "semantics.json".into(),
                    "--raw-dir".into(),
                    "raw".into(),
                ],
                "verify-strategy-input",
                &["--raw-dir", "--semantics-manifest", "--exact-dir"],
            )
            .expect("valid V2 strategy-input authority paths"),
            vec![
                PathBuf::from("raw"),
                PathBuf::from("semantics.json"),
                PathBuf::from("exact"),
            ]
        );
    }
}
