//! Thin CLI wrapper for the offline-only ACE-EV V2 evaluator.

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use clap::{Parser, Subcommand};
use ghost_launcher::ace_ev_v2_probe::{
    freeze_feature_scale, run_ace_ev_v2_evaluator, AceEvV2CaptureKind, AceEvV2EvaluateArgs,
    AceEvV2FreezeScaleArgs,
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
        } => {
            let summary = run_ace_ev_v2_evaluator(AceEvV2EvaluateArgs {
                events_dir,
                manifest_path: manifest,
                contract_path: contract,
                feature_scale_path: feature_scale,
                output_dir,
                capture_kind: AceEvV2CaptureKind::from_str(&capture_kind)?,
            })?;
            println!(
                "{} capture_status={} enrolled={} terminal_outcomes={}",
                summary.terminal_status,
                summary.capture_status,
                summary.enrolled_count,
                summary.terminal_outcome_count
            );
        }
    }
    Ok(())
}
