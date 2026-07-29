//! Thin CLI wrapper for the offline ACE Core one-day probe.

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use clap::Parser;
use ghost_launcher::ace_core_one_day_probe::{
    run_ace_core_one_day_probe, AceCoreOneDayProbeArgs, AceCoreProbeDayId,
};

#[derive(Debug, Parser)]
#[command(name = "ace_core_one_day_probe")]
#[command(about = "Offline-only ACE Core V3 one-day falsification probe")]
struct Cli {
    /// Directory containing immutable EventWriter exec_*.jsonl files.
    #[arg(long)]
    events_dir: PathBuf,
    /// Immutable RUG reality-capture manifest frozen at capture startup.
    #[arg(long)]
    manifest: PathBuf,
    /// New, empty directory for candidate rows, summary, and calibration.
    #[arg(long)]
    output_dir: PathBuf,
    /// `day1` freezes calibration; `day2` consumes the day1 file unchanged.
    #[arg(long)]
    day_id: String,
    /// Required only for day2: DAY1_PROBE_DIR/calibration_v1.json.
    #[arg(long)]
    calibration: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let summary = run_ace_core_one_day_probe(AceCoreOneDayProbeArgs {
        events_dir: cli.events_dir,
        manifest_path: cli.manifest,
        output_dir: cli.output_dir,
        day_id: AceCoreProbeDayId::from_str(&cli.day_id)?,
        calibration_path: cli.calibration,
    })?;
    println!(
        "{} run_id={} selected={} rest={} coverage_pct={:.2}",
        summary.terminal_status,
        summary.run_id,
        summary.metrics.selected_count,
        summary.metrics.rest_count,
        summary.metrics.evaluable_coverage_pct
    );
    Ok(())
}
