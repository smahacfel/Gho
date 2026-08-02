//! Thin CLI wrapper for the offline-only ACE-EV V2 evaluator.

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use clap::{Parser, Subcommand};
use ghost_launcher::ace_ev_v2_probe::{
    freeze_feature_scale, run_ace_ev_v2_evaluator, run_ace_ev_v2_monitor, AceEvV2CaptureKind,
    AceEvV2EvaluateArgs, AceEvV2FreezeScaleArgs, AceEvV2MonitorArgs,
};

#[derive(Debug, Parser)]
#[command(name = "ace_ev_v2_probe")]
#[command(about = "Offline-only ACE-EV V2 feature-scale and terminal-outcome evaluator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Freeze outcome-blind F1-F7 scaling from an immutable checkpoint tape.
    FreezeScale {
        #[arg(long)]
        events_dir: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        checkpoint_manifest: PathBuf,
        #[arg(long)]
        contract: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        /// Functional Git SHA of the offline evaluator used to produce the
        /// immutable scale artifact. This is explicit rather than inferred
        /// from a potentially dirty workspace.
        #[arg(long)]
        offline_evaluator_source_sha: String,
    },
    /// Evaluate V2 cohort enrollment and deterministic terminal outcomes.
    Evaluate {
        #[arg(long)]
        events_dir: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        contract: PathBuf,
        #[arg(long)]
        feature_scale: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long)]
        capture_kind: String,
        /// Required for capture-kind prospective_1000.
        #[arg(long)]
        amendment: Option<PathBuf>,
        /// Required when finalizing a TARGET_REACHED prospective capture.
        #[arg(long)]
        stop_evidence: Option<PathBuf>,
    },
    /// Monitor only newline-complete durable rows and create immutable
    /// prospective target evidence once the first 1,000 ordered outcomes are
    /// terminal.  This command has no RPC or runtime authority.
    Monitor {
        #[arg(long)]
        events_dir: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        contract: PathBuf,
        #[arg(long)]
        amendment: PathBuf,
        #[arg(long)]
        feature_scale: PathBuf,
        #[arg(long)]
        start_metrics: PathBuf,
        #[arg(long)]
        stop_evidence: PathBuf,
        #[arg(long, default_value_t = 1_000)]
        poll_interval_ms: u64,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::FreezeScale {
            events_dir,
            manifest,
            checkpoint_manifest,
            contract,
            output_dir,
            offline_evaluator_source_sha,
        } => {
            let scale = freeze_feature_scale(AceEvV2FreezeScaleArgs {
                events_dir,
                manifest_path: manifest,
                checkpoint_manifest_path: checkpoint_manifest,
                contract_path: contract,
                output_dir,
                offline_evaluator_source_sha,
            })?;
            println!(
                "FEATURE_SCALE_FROZEN population_count={} source_run_id={}",
                scale.population_count, scale.source_run_id
            );
        }
        Command::Evaluate {
            events_dir,
            manifest,
            contract,
            feature_scale,
            output_dir,
            capture_kind,
            amendment,
            stop_evidence,
        } => {
            let summary = run_ace_ev_v2_evaluator(AceEvV2EvaluateArgs {
                events_dir,
                manifest_path: manifest,
                contract_path: contract,
                feature_scale_path: feature_scale,
                output_dir,
                capture_kind: AceEvV2CaptureKind::from_str(&capture_kind)?,
                amendment_path: amendment,
                stop_evidence_path: stop_evidence,
            })?;
            println!(
                "{} capture_status={} enrolled={} terminal_outcomes={}",
                summary.terminal_status,
                summary.capture_status,
                summary.enrolled_count,
                summary.terminal_outcome_count
            );
        }
        Command::Monitor {
            events_dir,
            manifest,
            contract,
            amendment,
            feature_scale,
            start_metrics,
            stop_evidence,
            poll_interval_ms,
        } => {
            run_ace_ev_v2_monitor(AceEvV2MonitorArgs {
                events_dir,
                manifest_path: manifest,
                contract_path: contract,
                amendment_path: amendment,
                feature_scale_path: feature_scale,
                start_metrics_path: start_metrics,
                stop_evidence_path: stop_evidence,
                poll_interval_ms,
            })?;
            println!("ACE_EV_V2_PROSPECTIVE_TARGET_REACHED");
        }
    }
    Ok(())
}
