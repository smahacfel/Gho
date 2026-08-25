//! Standalone entry point for Pump Research Evidence Tape V1.
//!
//! PR-A owns immutable raw capture. PR-B adds read-only certification from a
//! closed raw run; it never opens a source stream or changes active Seer
//! runtime behavior.

use anyhow::{bail, Context, Result};
use ghost_core::pump_research_tape::{
    PumpResearchRequiredEvidenceV1, PumpResearchTapeQualificationStatusV1,
};
use seer::research_tape::{
    create_operator_preflight_from_config_path, run_capture_from_config_path,
};
use seer::research_tape_materializer::{
    certify_pump_research_raw_run_v1, certify_pump_research_verified_go_d_v1,
    export_pump_research_windows_v1, PumpResearchWindowExportRequestV1,
    PumpResearchWindowTimeAxisV1,
};
use std::{env, path::PathBuf};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pump_research_tape=info,seer=info,warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut arguments = env::args_os();
    let executable = arguments
        .next()
        .unwrap_or_else(|| "pump-research-tape".into());
    let Some(command) = arguments.next() else {
        print_usage(&executable);
        bail!("missing Pump Research Tape command");
    };
    let command = command
        .into_string()
        .map_err(|_| anyhow::anyhow!("command is not valid UTF-8"))?;

    match command.as_str() {
        "preflight" => {
            let arguments: Vec<_> = arguments.collect();
            if is_command_help(&arguments) {
                print_usage(&executable);
                return Ok(());
            }
            let arguments = parse_preflight_arguments(arguments)?;
            let summary = create_operator_preflight_from_config_path(
                &arguments.config_path,
                &arguments.output_dir,
            )?;
            info!(
                bundle_dir = %summary.bundle_dir.display(),
                receipt_path = %summary.receipt_path.display(),
                release_binary_sha256 = %summary.release_binary_digest.sha256,
                provenance_fingerprint_blake3 = %summary.artifact_provenance_fingerprint.blake3,
                "Pump Research operator preflight sealed; capture must reference this receipt"
            );
            Ok(())
        }
        "capture" => {
            let arguments: Vec<_> = arguments.collect();
            if is_command_help(&arguments) {
                print_usage(&executable);
                return Ok(());
            }
            let arguments = parse_capture_arguments(arguments)?;
            let summary = run_capture_from_config_path(
                &arguments.config_path,
                &arguments.provenance_receipt_path,
            )
            .await?;
            if !summary.is_complete() {
                error!(
                    run_id = %summary.run_id,
                    raw_dir = %summary.raw_dir.display(),
                    status = ?summary.status,
                    gap_count = summary.gap_count,
                    source_error = ?summary.source_error,
                    writer_error = ?summary.writer_error,
                    completion_receipt_error = ?summary.completion_receipt_error,
                    "Pump Research capture retained raw evidence but failed its required V1 completion contract"
                );
                bail!(
                    "Pump Research capture is not complete; inspect {}/run_completion_receipt.json",
                    summary.raw_dir.display()
                );
            }
            info!(
                run_id = %summary.run_id,
                raw_dir = %summary.raw_dir.display(),
                "Pump Research Tape raw capture completed"
            );
            Ok(())
        }
        "provider-suitability" => {
            let arguments: Vec<_> = arguments.collect();
            if is_command_help(&arguments) {
                print_usage(&executable);
                return Ok(());
            }
            let _ = arguments;
            bail!(
                "GO-E external provider audit is retired and is not a GO-D source or promotion gate"
            )
        }
        "certify" => {
            let arguments: Vec<_> = arguments.collect();
            if is_command_help(&arguments) {
                print_usage(&executable);
                return Ok(());
            }
            let arguments = parse_certify_arguments(arguments)?;
            if arguments.qualification_audit_config_path.is_some() {
                bail!(
                    "GO-E external provider audit is retired; certify GO-D only through a hash-pinned frozen-tape authority"
                );
            }
            let summary = match arguments.go_d_source_authority_path.as_deref() {
                Some(source_authority_path) => certify_pump_research_verified_go_d_v1(
                    &arguments.run_dir,
                    &arguments.output_dir,
                    source_authority_path,
                    arguments
                        .expected_go_d_source_authority_sha256
                        .as_deref()
                        .ok_or_else(|| {
                            anyhow::anyhow!("certify lost expected GO-D authority SHA-256")
                        })?,
                )?,
                None => {
                    certify_pump_research_raw_run_v1(&arguments.run_dir, &arguments.output_dir)?
                }
            };
            info!(
                source_run_id = %summary.source_run_id,
                output_dir = %summary.output_dir.display(),
                qualification_status = ?summary.qualification_status,
                qualification_status_label = certification_qualification_status_label(
                    summary.qualification_status
                ),
                transaction_count = summary.transaction_count,
                trajectory_count = summary.trajectory_count,
                exact_trajectory_count = summary.exact_trajectory_count,
                successful_rooted_mutation_count = summary.successful_rooted_mutation_count,
                exact_rooted_mutation_count = summary.exact_rooted_mutation_count,
                birth_count = summary.birth_count,
                "Pump Research raw run certification completed"
            );
            Ok(())
        }
        "export-window" => {
            let arguments: Vec<_> = arguments.collect();
            if is_command_help(&arguments) {
                print_usage(&executable);
                return Ok(());
            }
            let arguments = parse_export_window_arguments(arguments)?;
            let summary = export_pump_research_windows_v1(
                &arguments.tape_dir,
                PumpResearchWindowExportRequestV1 {
                    time_axis: arguments.time_axis,
                    observation_ms: arguments.observation_ms,
                    forward_ms: arguments.forward_ms,
                    required_evidence: arguments.required_evidence,
                },
                &arguments.output_dir,
            )?;
            info!(
                source_run_id = %summary.source_run_id,
                output_dir = %summary.output_dir.display(),
                exported_birth_count = summary.exported_birth_count,
                complete_window_count = summary.complete_window_count,
                "Pump Research generic launch-window export completed"
            );
            Ok(())
        }
        "--help" | "-h" | "help" => {
            print_usage(&executable);
            Ok(())
        }
        _ => {
            print_usage(&executable);
            bail!("unknown Pump Research Tape command {command:?}")
        }
    }
}

const fn certification_qualification_status_label(
    status: PumpResearchTapeQualificationStatusV1,
) -> &'static str {
    match status {
        PumpResearchTapeQualificationStatusV1::Unqualified => "unqualified",
        PumpResearchTapeQualificationStatusV1::Ready => "ready",
        PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape => "verified_frozen_tape",
        PumpResearchTapeQualificationStatusV1::Blocked(_) => "blocked",
    }
}

fn is_command_help(arguments: &[std::ffi::OsString]) -> bool {
    matches!(arguments, [argument] if argument == "--help" || argument == "-h")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CaptureArguments {
    config_path: PathBuf,
    provenance_receipt_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreflightArguments {
    config_path: PathBuf,
    output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CertifyArguments {
    run_dir: PathBuf,
    output_dir: PathBuf,
    qualification_audit_config_path: Option<PathBuf>,
    provider_suitability_receipt_path: Option<PathBuf>,
    provider_independence_attestation_path: Option<PathBuf>,
    expected_provider_independence_sha256: Option<String>,
    go_d_source_authority_path: Option<PathBuf>,
    expected_go_d_source_authority_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExportWindowArguments {
    tape_dir: PathBuf,
    time_axis: PumpResearchWindowTimeAxisV1,
    observation_ms: u64,
    forward_ms: u64,
    required_evidence: Option<PumpResearchRequiredEvidenceV1>,
    output_dir: PathBuf,
}

fn parse_named_path_arguments(
    arguments: Vec<std::ffi::OsString>,
    command: &str,
    required_flags: &[&str],
) -> Result<Vec<PathBuf>> {
    let mut values: Vec<Option<PathBuf>> = vec![None; required_flags.len()];
    let mut iterator = arguments.into_iter();
    while let Some(flag) = iterator.next() {
        let flag = flag
            .into_string()
            .map_err(|_| anyhow::anyhow!("{command} flag is not valid UTF-8"))?;
        let Some(position) = required_flags.iter().position(|expected| *expected == flag) else {
            bail!("{command} accepts only {}", required_flags.join(" and "));
        };
        if values[position].is_some() {
            bail!("{command} received duplicate {flag}")
        }
        let Some(path) = iterator.next() else {
            bail!("{command} requires a path after {flag}")
        };
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty() {
            bail!("{command} path after {flag} must not be empty")
        }
        values[position] = Some(path);
    }
    values
        .into_iter()
        .enumerate()
        .map(|(position, value)| {
            value.ok_or_else(|| {
                anyhow::anyhow!("{command} requires {} <path>", required_flags[position])
            })
        })
        .collect()
}

fn parse_capture_arguments(arguments: Vec<std::ffi::OsString>) -> Result<CaptureArguments> {
    let mut values =
        parse_named_path_arguments(arguments, "capture", &["--config", "--provenance-receipt"])?
            .into_iter();
    Ok(CaptureArguments {
        config_path: values
            .next()
            .ok_or_else(|| anyhow::anyhow!("capture parser lost --config"))?,
        provenance_receipt_path: values
            .next()
            .ok_or_else(|| anyhow::anyhow!("capture parser lost --provenance-receipt"))?,
    })
}

fn parse_preflight_arguments(arguments: Vec<std::ffi::OsString>) -> Result<PreflightArguments> {
    let mut values =
        parse_named_path_arguments(arguments, "preflight", &["--config", "--output"])?.into_iter();
    Ok(PreflightArguments {
        config_path: values
            .next()
            .ok_or_else(|| anyhow::anyhow!("preflight parser lost --config"))?,
        output_dir: values
            .next()
            .ok_or_else(|| anyhow::anyhow!("preflight parser lost --output"))?,
    })
}

fn parse_certify_arguments(arguments: Vec<std::ffi::OsString>) -> Result<CertifyArguments> {
    let mut run_dir = None;
    let mut output_dir = None;
    let mut qualification_audit_config_path = None;
    let mut provider_suitability_receipt_path = None;
    let mut provider_independence_attestation_path = None;
    let mut expected_provider_independence_sha256 = None;
    let mut go_d_source_authority_path = None;
    let mut expected_go_d_source_authority_sha256 = None;
    let mut iterator = arguments.into_iter();
    while let Some(flag) = iterator.next() {
        let flag = flag
            .into_string()
            .map_err(|_| anyhow::anyhow!("certify flag is not valid UTF-8"))?;
        let Some(value) = iterator.next() else {
            bail!("certify requires a path after {flag}");
        };
        match flag.as_str() {
            "--expected-provider-independence-sha256"
                if expected_provider_independence_sha256.is_none() =>
            {
                expected_provider_independence_sha256 = Some(value.into_string().map_err(|_| {
                    anyhow::anyhow!("certify expected provider-independence SHA-256 is not UTF-8")
                })?)
            }
            "--expected-provider-independence-sha256" => {
                bail!("certify received duplicate {flag}")
            }
            "--expected-go-d-source-authority-sha256"
                if expected_go_d_source_authority_sha256.is_none() =>
            {
                expected_go_d_source_authority_sha256 = Some(value.into_string().map_err(|_| {
                    anyhow::anyhow!("certify expected GO-D authority SHA-256 is not UTF-8")
                })?)
            }
            "--expected-go-d-source-authority-sha256" => {
                bail!("certify received duplicate {flag}")
            }
            _ => {
                let path = PathBuf::from(value);
                if path.as_os_str().is_empty() {
                    bail!("certify path after {flag} must not be empty");
                }
                match flag.as_str() {
                    "--run-dir" if run_dir.is_none() => run_dir = Some(path),
                    "--output" if output_dir.is_none() => output_dir = Some(path),
                    "--qualification-audit-config" if qualification_audit_config_path.is_none() => {
                        qualification_audit_config_path = Some(path)
                    }
                    "--provider-suitability-receipt"
                        if provider_suitability_receipt_path.is_none() =>
                    {
                        provider_suitability_receipt_path = Some(path)
                    }
                    "--provider-independence-attestation"
                        if provider_independence_attestation_path.is_none() =>
                    {
                        provider_independence_attestation_path = Some(path)
                    }
                    "--go-d-source-authority" if go_d_source_authority_path.is_none() => {
                        go_d_source_authority_path = Some(path)
                    }
                    "--run-dir"
                    | "--output"
                    | "--qualification-audit-config"
                    | "--provider-suitability-receipt"
                    | "--provider-independence-attestation"
                    | "--go-d-source-authority" => {
                        bail!("certify received duplicate {flag}")
                    }
                    _ => bail!("certify received unsupported flag {flag:?}"),
                }
            }
        }
    }
    let qualified_authority_complete = provider_suitability_receipt_path.is_some()
        && provider_independence_attestation_path.is_some()
        && expected_provider_independence_sha256.is_some();
    let any_qualified_authority = provider_suitability_receipt_path.is_some()
        || provider_independence_attestation_path.is_some()
        || expected_provider_independence_sha256.is_some();
    if (qualification_audit_config_path.is_some() && !qualified_authority_complete)
        || (qualification_audit_config_path.is_none() && any_qualified_authority)
    {
        bail!(
            "qualified certify requires --qualification-audit-config, --provider-suitability-receipt, --provider-independence-attestation and --expected-provider-independence-sha256 together"
        );
    }
    let go_d_authority_complete =
        go_d_source_authority_path.is_some() && expected_go_d_source_authority_sha256.is_some();
    let any_go_d_authority =
        go_d_source_authority_path.is_some() || expected_go_d_source_authority_sha256.is_some();
    if any_go_d_authority && !go_d_authority_complete {
        bail!(
            "verified GO-D certify requires --go-d-source-authority and --expected-go-d-source-authority-sha256 together"
        );
    }
    if any_go_d_authority && (qualification_audit_config_path.is_some() || any_qualified_authority)
    {
        bail!("verified GO-D authority and retired GO-E audit authority are mutually exclusive");
    }
    Ok(CertifyArguments {
        run_dir: run_dir.ok_or_else(|| anyhow::anyhow!("certify requires --run-dir <path>"))?,
        output_dir: output_dir
            .ok_or_else(|| anyhow::anyhow!("certify requires --output <path>"))?,
        qualification_audit_config_path,
        provider_suitability_receipt_path,
        provider_independence_attestation_path,
        expected_provider_independence_sha256,
        go_d_source_authority_path,
        expected_go_d_source_authority_sha256,
    })
}

fn parse_export_window_arguments(
    arguments: Vec<std::ffi::OsString>,
) -> Result<ExportWindowArguments> {
    let mut tape_dir = None;
    let mut time_axis = None;
    let mut observation_ms = None;
    let mut forward_ms = None;
    let mut required_evidence = None;
    let mut output_dir = None;
    let mut iterator = arguments.into_iter();
    while let Some(flag) = iterator.next() {
        let flag = flag
            .into_string()
            .map_err(|_| anyhow::anyhow!("export-window flag is not valid UTF-8"))?;
        let Some(value) = iterator.next() else {
            bail!("export-window requires a value after {flag}");
        };
        let value = value
            .into_string()
            .map_err(|_| anyhow::anyhow!("export-window value after {flag} is not valid UTF-8"))?;
        match flag.as_str() {
            "--tape" if tape_dir.is_none() => tape_dir = Some(PathBuf::from(value)),
            "--time-axis" if time_axis.is_none() => {
                time_axis = Some(PumpResearchWindowTimeAxisV1::parse_cli(&value)?)
            }
            "--observation-ms" if observation_ms.is_none() => {
                observation_ms = Some(parse_positive_millis("--observation-ms", &value)?)
            }
            "--forward-ms" if forward_ms.is_none() => {
                forward_ms = Some(parse_positive_millis("--forward-ms", &value)?)
            }
            "--require-evidence" if required_evidence.is_none() => {
                required_evidence = Some(match value.as_str() {
                    "participant_balance" => PumpResearchRequiredEvidenceV1::ParticipantBalance,
                    _ => bail!(
                        "--require-evidence accepts only 'participant_balance', got {value:?}"
                    ),
                })
            }
            "--output" if output_dir.is_none() => output_dir = Some(PathBuf::from(value)),
            "--tape" | "--time-axis" | "--observation-ms" | "--forward-ms"
            | "--require-evidence" | "--output" => {
                bail!("export-window received duplicate {flag}")
            }
            _ => bail!("export-window received unsupported flag {flag:?}"),
        }
    }
    Ok(ExportWindowArguments {
        tape_dir: tape_dir
            .ok_or_else(|| anyhow::anyhow!("export-window requires --tape <path>"))?,
        time_axis: time_axis
            .ok_or_else(|| anyhow::anyhow!("export-window requires --time-axis chain|observed"))?,
        observation_ms: observation_ms
            .ok_or_else(|| anyhow::anyhow!("export-window requires --observation-ms <u64>"))?,
        forward_ms: forward_ms
            .ok_or_else(|| anyhow::anyhow!("export-window requires --forward-ms <u64>"))?,
        required_evidence,
        output_dir: output_dir
            .ok_or_else(|| anyhow::anyhow!("export-window requires --output <path>"))?,
    })
}

fn parse_positive_millis(flag: &str, value: &str) -> Result<u64> {
    let value = value
        .parse::<u64>()
        .with_context(|| format!("{flag} must be an unsigned integer in milliseconds"))?;
    if value == 0 {
        bail!("{flag} must be greater than zero");
    }
    Ok(value)
}

fn print_usage(executable: &std::ffi::OsStr) {
    eprintln!(
        "Usage (run from the repository root):\n  {} preflight --config /protected/operator/pump-research-tape-v1.toml --output datasets/pump-research/preflight/<id>\n  datasets/pump-research/preflight/<id>/release/pump-research-tape capture --config /protected/operator/pump-research-tape-v1.toml --provenance-receipt datasets/pump-research/preflight/<id>/operator_preflight_receipt_v1.json\n  pump-research-tape certify --run-dir datasets/pump-research/<run-id>/raw --output datasets/pump-research/<run-id>/exact-go-d-verified-v1 --go-d-source-authority configs/rollout/pump-research-go-d-source-authority-v1.json --expected-go-d-source-authority-sha256 <hex>\n  pump-research-tape export-window --tape datasets/pump-research/<run-id>/exact-go-d-verified-v1 --time-axis observed --observation-ms 150000 --forward-ms 180000 --require-evidence participant_balance --output datasets/experiments/<strategy>/<run-id>\n\nThe bootstrap must be a non-debug release binary. `preflight` seals a fresh `cargo build --locked --offline --release` binary from a full source snapshot; `capture` must execute that copied sealed binary. GO-D is the verified frozen source authority. The promoted offline `certify` path requires a hash-pinned GO-D authority receipt and performs no RPC or Yellowstone I/O. Plain `certify` remains a development-only Unqualified materialization. GO-E/provider-suitability and audit-backed certify are retired and fail before provider I/O. `export-window` requires VerifiedFrozenTape with GO_D_SOURCE_AUTHORITY=VERIFIED and EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE=true; it never mixes chain and observed time.",
        executable.to_string_lossy(),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        certification_qualification_status_label, is_command_help, parse_capture_arguments,
        parse_certify_arguments, parse_export_window_arguments, parse_preflight_arguments,
        CaptureArguments, CertifyArguments, ExportWindowArguments, PreflightArguments,
        PumpResearchRequiredEvidenceV1, PumpResearchTapeQualificationStatusV1,
        PumpResearchWindowTimeAxisV1,
    };
    use std::ffi::OsString;

    #[test]
    fn capture_cli_requires_config_and_immutable_provenance_receipt() {
        assert_eq!(
            parse_capture_arguments(vec![
                "--provenance-receipt".into(),
                "preflight.json".into(),
                "--config".into(),
                "capture.toml".into(),
            ])
            .expect("valid capture arguments"),
            CaptureArguments {
                config_path: std::path::PathBuf::from("capture.toml"),
                provenance_receipt_path: std::path::PathBuf::from("preflight.json"),
            }
        );
        assert!(parse_capture_arguments(Vec::<OsString>::new()).is_err());
        assert!(parse_capture_arguments(vec!["--other".into(), "capture.toml".into()]).is_err());
        assert!(parse_capture_arguments(vec!["--config".into()]).is_err());
        assert!(parse_capture_arguments(vec![
            "--config".into(),
            "capture.toml".into(),
            "--config".into(),
            "duplicate.toml".into(),
        ])
        .is_err());
    }

    #[test]
    fn subcommand_help_is_accepted_without_fake_required_path() {
        assert!(is_command_help(&[OsString::from("--help")]));
        assert!(is_command_help(&[OsString::from("-h")]));
        assert!(!is_command_help(&[OsString::from("--run-dir")]));
        assert!(!is_command_help(&[
            OsString::from("--help"),
            OsString::from("extra"),
        ]));
    }

    #[test]
    fn preflight_cli_requires_config_and_new_output_directory() {
        assert_eq!(
            parse_preflight_arguments(vec![
                "--output".into(),
                "preflight-output".into(),
                "--config".into(),
                "capture.toml".into(),
            ])
            .expect("valid preflight arguments"),
            PreflightArguments {
                config_path: std::path::PathBuf::from("capture.toml"),
                output_dir: std::path::PathBuf::from("preflight-output"),
            }
        );
        assert!(parse_preflight_arguments(Vec::<OsString>::new()).is_err());
        assert!(parse_preflight_arguments(vec!["--config".into(), "capture.toml".into()]).is_err());
    }

    #[test]
    fn certify_cli_requires_closed_raw_run_and_new_output_directory() {
        assert_eq!(
            parse_certify_arguments(vec![
                "--output".into(),
                "exact".into(),
                "--run-dir".into(),
                "raw".into(),
            ])
            .expect("valid certify arguments"),
            CertifyArguments {
                run_dir: std::path::PathBuf::from("raw"),
                output_dir: std::path::PathBuf::from("exact"),
                qualification_audit_config_path: None,
                provider_suitability_receipt_path: None,
                provider_independence_attestation_path: None,
                expected_provider_independence_sha256: None,
                go_d_source_authority_path: None,
                expected_go_d_source_authority_sha256: None,
            }
        );
        assert_eq!(
            parse_certify_arguments(vec![
                "--run-dir".into(),
                "raw".into(),
                "--qualification-audit-config".into(),
                "audit.toml".into(),
                "--provider-suitability-receipt".into(),
                "go-e0.json".into(),
                "--provider-independence-attestation".into(),
                "provider_independence_attestation_v1.json".into(),
                "--expected-provider-independence-sha256".into(),
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                "--output".into(),
                "exact".into(),
            ])
            .expect("valid qualified certify arguments"),
            CertifyArguments {
                run_dir: std::path::PathBuf::from("raw"),
                output_dir: std::path::PathBuf::from("exact"),
                qualification_audit_config_path: Some(std::path::PathBuf::from("audit.toml")),
                provider_suitability_receipt_path: Some(std::path::PathBuf::from("go-e0.json")),
                provider_independence_attestation_path: Some(std::path::PathBuf::from(
                    "provider_independence_attestation_v1.json",
                )),
                expected_provider_independence_sha256: Some(
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
                ),
                go_d_source_authority_path: None,
                expected_go_d_source_authority_sha256: None,
            }
        );
        assert_eq!(
            parse_certify_arguments(vec![
                "--run-dir".into(),
                "raw".into(),
                "--output".into(),
                "exact-go-d".into(),
                "--go-d-source-authority".into(),
                "go-d.json".into(),
                "--expected-go-d-source-authority-sha256".into(),
                "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".into(),
            ])
            .expect("valid verified GO-D certify arguments"),
            CertifyArguments {
                run_dir: std::path::PathBuf::from("raw"),
                output_dir: std::path::PathBuf::from("exact-go-d"),
                qualification_audit_config_path: None,
                provider_suitability_receipt_path: None,
                provider_independence_attestation_path: None,
                expected_provider_independence_sha256: None,
                go_d_source_authority_path: Some(std::path::PathBuf::from("go-d.json")),
                expected_go_d_source_authority_sha256: Some(
                    "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".to_owned(),
                ),
            }
        );
        assert!(parse_certify_arguments(Vec::<OsString>::new()).is_err());
        assert!(parse_certify_arguments(vec!["--run-dir".into(), "raw".into()]).is_err());
        assert!(parse_certify_arguments(vec![
            "--run-dir".into(),
            "raw".into(),
            "--output".into(),
            "exact".into(),
            "--qualification-audit-config".into(),
            "audit.toml".into(),
        ])
        .is_err());
        assert!(parse_certify_arguments(vec![
            "--run-dir".into(),
            "raw".into(),
            "--output".into(),
            "exact".into(),
            "--go-d-source-authority".into(),
            "go-d.json".into(),
        ])
        .is_err());
        assert!(parse_certify_arguments(vec![
            "--run-dir".into(),
            "raw".into(),
            "--output".into(),
            "exact".into(),
            "--go-d-source-authority".into(),
            "go-d.json".into(),
            "--expected-go-d-source-authority-sha256".into(),
            "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".into(),
            "--qualification-audit-config".into(),
            "audit.toml".into(),
        ])
        .is_err());
        assert!(parse_certify_arguments(vec![
            "--run-dir".into(),
            "raw".into(),
            "--output".into(),
            "exact".into(),
            "--provider-independence-attestation".into(),
            "attestation.json".into(),
        ])
        .is_err());
    }

    #[test]
    fn certification_log_label_matches_the_exact_manifest_status() {
        use ghost_core::pump_research_tape::PumpResearchQualificationBlockerV1;

        assert_eq!(
            certification_qualification_status_label(
                PumpResearchTapeQualificationStatusV1::Unqualified
            ),
            "unqualified"
        );
        assert_eq!(
            certification_qualification_status_label(PumpResearchTapeQualificationStatusV1::Ready),
            "ready"
        );
        assert_eq!(
            certification_qualification_status_label(
                PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape
            ),
            "verified_frozen_tape"
        );
        assert_eq!(
            certification_qualification_status_label(
                PumpResearchTapeQualificationStatusV1::Blocked(
                    PumpResearchQualificationBlockerV1::SourceCoverageUnproven,
                )
            ),
            "blocked"
        );
    }

    #[test]
    fn export_window_cli_requires_explicit_time_axis_and_optional_typed_evidence() {
        assert_eq!(
            parse_export_window_arguments(vec![
                "--tape".into(),
                "exact".into(),
                "--time-axis".into(),
                "observed".into(),
                "--observation-ms".into(),
                "150000".into(),
                "--forward-ms".into(),
                "180000".into(),
                "--require-evidence".into(),
                "participant_balance".into(),
                "--output".into(),
                "window".into(),
            ])
            .expect("valid export arguments"),
            ExportWindowArguments {
                tape_dir: std::path::PathBuf::from("exact"),
                time_axis: PumpResearchWindowTimeAxisV1::Observed,
                observation_ms: 150_000,
                forward_ms: 180_000,
                required_evidence: Some(PumpResearchRequiredEvidenceV1::ParticipantBalance),
                output_dir: std::path::PathBuf::from("window"),
            }
        );
        assert!(parse_export_window_arguments(vec![
            "--tape".into(),
            "exact".into(),
            "--time-axis".into(),
            "mixed".into(),
        ])
        .is_err());
        assert!(parse_export_window_arguments(vec![
            "--tape".into(),
            "exact".into(),
            "--time-axis".into(),
            "chain".into(),
            "--observation-ms".into(),
            "0".into(),
            "--forward-ms".into(),
            "1".into(),
            "--output".into(),
            "window".into(),
        ])
        .is_err());
    }
}
