use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use ghost_brain::guardian::post_buy::shadow_v2::{
    ClockDomain, ClockedTimestamp, EventOrderComponent, EventOrderKey, ShadowPathSampleV2,
    ShadowPathSamplerConfigV2, ShadowPathSamplingModeV2, ShadowPathSamplingReasonV2,
    ShadowV2Envelope, ShadowV2Record, ShadowV2ValidationHarness, ShadowV2ValidationHarnessConfig,
    ShadowV2WriteStatus,
};
use serde_json::json;

const DEFAULT_RUN_ID: &str = "shadow-v2-l2-d3b-runtime-harness-density-emission-20260705-r1";
const DEFAULT_SCOPE_ROOT: &str =
    "reports/selector/shadow-v2-l2-d3b-runtime-harness-density-emission-20260705-r1";

#[derive(Debug, Parser)]
#[command(
    name = "shadow-v2-l2-d3b-runtime-harness-density-scope",
    about = "Generate an L2-D3B density scope through ShadowV2ValidationHarness"
)]
struct Args {
    #[arg(long, default_value = DEFAULT_SCOPE_ROOT)]
    scope_root: PathBuf,

    #[arg(long, default_value = DEFAULT_RUN_ID)]
    run_id: String,

    #[arg(long, default_value_t = 121_000)]
    duration_ms: u64,

    #[arg(long, default_value_t = 1_000)]
    sample_interval_ms: u64,

    #[arg(long, default_value_t = false)]
    overwrite: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.sample_interval_ms == 0 {
        bail!("sample_interval_ms must be positive");
    }
    if args.duration_ms < 121_000 {
        bail!("duration_ms must cover the 121000ms L2-D3B retention contract");
    }

    fs::create_dir_all(&args.scope_root)
        .with_context(|| format!("failed to create scope root {}", args.scope_root.display()))?;

    let paths = D3bPaths::new(&args.scope_root);
    paths.prepare(args.overwrite)?;

    let config = ShadowV2ValidationHarnessConfig::new(
        args.run_id.clone(),
        paths.canonical.clone(),
        paths.replay.clone(),
        paths.lifecycle.clone(),
        paths.density.clone(),
    );
    if config.path_sampler_config != ShadowPathSamplerConfigV2::standard_120s() {
        bail!("validation harness is not using standard_120s sampler");
    }
    if config.path_sampler_config.max_horizon_ms != 121_000 {
        bail!(
            "standard_120s max_horizon_ms is {}, expected 121000",
            config.path_sampler_config.max_horizon_ms
        );
    }

    let mut harness = ShadowV2ValidationHarness::new(config)
        .context("failed to initialize ShadowV2ValidationHarness")?;
    let sample_count = args.duration_ms / args.sample_interval_ms + 1;
    let mut final_event_id = String::new();

    for idx in 0..sample_count {
        let age_ms = idx * args.sample_interval_ms;
        let event_id = format!("d3b-path-sample-{idx:03}");
        final_event_id = event_id.clone();
        let sample = path_sample(&args.run_id, &event_id, age_ms, idx as i32);
        let outcome = harness.append_record(ShadowV2Record::ShadowPathSampleV2(sample));
        if outcome.canonical_write != ShadowV2WriteStatus::Ok {
            bail!(
                "canonical write failed for {event_id}: {:?}",
                outcome.canonical_write
            );
        }
        if outcome.density_write != ShadowV2WriteStatus::Ok {
            bail!(
                "density write failed for {event_id}: {:?}",
                outcome.density_write
            );
        }
    }

    let manifest = json!({
        "stage": "L2-D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF",
        "final_verdict": "L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F",
        "run_id": args.run_id,
        "duration_ms": args.duration_ms,
        "configured_run_seconds": args.duration_ms / 1_000,
        "sample_interval_ms": args.sample_interval_ms,
        "path_sample_count": sample_count,
        "final_high_watermark": final_event_id,
        "canonical_path_sample_rows": sample_count,
        "density_rows_expected": sample_count * 7,
        "canonical_event_stream": paths.canonical,
        "path_density_stream": paths.density,
        "density_rows_written_directly": false,
        "density_derivation_source": "ShadowV2ValidationHarness::append_record",
        "canonical_source_schema": "shadow_position_event_v2",
        "density_schema": "shadow_path_density_v2",
        "declared_supported_horizons_ms": [2000, 3000, 10000, 30000, 120000],
        "unsupported_horizons_ms": [300000, 500000],
        "retention_contract_ms": 121000,
        "required_replay_coverage_ms": 121000,
        "runtime_harness_density_emission_proof": true,
        "live_runtime_density_emission_proof": false,
        "l2_f_allowed_next": true,
        "runtime_approval": false,
        "research_grade": false,
        "live_equivalence": false,
        "strategy_research_unblocked": false,
        "shadow_close_only": false,
        "active_close": false
    });
    fs::write(&paths.manifest, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", paths.manifest.display()))?;

    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}

#[derive(Debug)]
struct D3bPaths {
    canonical: PathBuf,
    replay: PathBuf,
    lifecycle: PathBuf,
    density: PathBuf,
    manifest: PathBuf,
}

impl D3bPaths {
    fn new(scope_root: &PathBuf) -> Self {
        Self {
            canonical: scope_root.join("shadow_position_event_v2.jsonl"),
            replay: scope_root.join("shadow_replay_v2.jsonl"),
            lifecycle: scope_root.join("shadow_lifecycle_v2.jsonl"),
            density: scope_root.join("shadow_path_density_v2.jsonl"),
            manifest: scope_root.join("d3b_runtime_harness_density_manifest.json"),
        }
    }

    fn prepare(&self, overwrite: bool) -> Result<()> {
        let outputs = [
            &self.canonical,
            &self.replay,
            &self.lifecycle,
            &self.density,
            &self.manifest,
        ];
        let existing = outputs
            .iter()
            .filter(|path| path.exists())
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        if !existing.is_empty() && !overwrite {
            bail!(
                "output files already exist; rerun with --overwrite for known D3B outputs only: {}",
                existing.join(", ")
            );
        }
        if overwrite {
            for path in outputs {
                if path.exists() {
                    fs::remove_file(path)
                        .with_context(|| format!("failed to remove {}", path.display()))?;
                }
            }
        }
        Ok(())
    }
}

fn path_sample(run_id: &str, event_id: &str, age_ms: u64, pnl_mark_bps: i32) -> ShadowPathSampleV2 {
    let observed_at_wall_ms = 1_785_000_000_123_u64.saturating_add(age_ms);
    ShadowPathSampleV2 {
        envelope: envelope(run_id, event_id, observed_at_wall_ms),
        event_order_key: event_order_key(age_ms, observed_at_wall_ms),
        sampling_mode: ShadowPathSamplingModeV2::Standard120s,
        path_horizon_ms: ShadowPathSamplerConfigV2::standard_120s().max_horizon_ms,
        sample_ts_ms: ClockedTimestamp {
            field_name: "sample_ts_ms".to_string(),
            value: Some(observed_at_wall_ms as i64),
            clock_domain: ClockDomain::StreamObservedMs,
            clock_source: "D3B_RUNTIME_HARNESS_SCOPE".to_string(),
            causal_boundary: "PATH_SAMPLE".to_string(),
        },
        sample_slot: Some(45),
        age_ms,
        pool_state_ref: "d3b-pool-state-source".to_string(),
        mark_price: Some(0.00003),
        executable_exit_quote: None,
        pnl_mark_bps: Some(pnl_mark_bps),
        pnl_executable_bps: None,
        mfe_mark_bps: Some(pnl_mark_bps),
        mae_mark_bps: Some(pnl_mark_bps),
        source_quality: "D3B_RUNTIME_HARNESS_DENSITY_SCOPE".to_string(),
        sampling_reason: ShadowPathSamplingReasonV2::Heartbeat.label().to_string(),
        exact_or_approx: "APPROX_AMBIGUOUS_EVENT_ORDER".to_string(),
        truncated: false,
    }
}

fn envelope(run_id: &str, event_id: &str, produced_at_ms: u64) -> ShadowV2Envelope {
    let mut envelope = ShadowV2Envelope::contract_header(
        "shadow_path_sample_v2",
        run_id,
        "d3b-position-001",
        event_id,
        "d3b-pool-001",
        "d3b-base-mint-001",
    );
    envelope.session_id = Some("d3b-session-001".to_string());
    envelope.candidate_id = Some("d3b-candidate-001".to_string());
    envelope.produced_at_ms = produced_at_ms;
    envelope.produced_at_slot = Some(45);
    envelope.temporal_class = ghost_brain::guardian::post_buy::shadow_v2::TemporalClass::PostEntry;
    envelope.clock_domain = ClockDomain::StreamObservedMs;
    envelope.quality = "D3B_RUNTIME_HARNESS_DENSITY_SCOPE".to_string();
    envelope
}

fn event_order_key(age_ms: u64, observed_at_wall_ms: u64) -> EventOrderKey {
    EventOrderKey {
        slot: EventOrderComponent::known(45),
        block_time: EventOrderComponent::known(1_785_000_000_i64 + (age_ms / 1_000) as i64),
        signature: EventOrderComponent::known(format!("d3b-sig-{age_ms}")),
        transaction_index_or_unknown: EventOrderComponent::known((age_ms / 1_000) as u32),
        instruction_index_or_unknown: EventOrderComponent::known(0),
        inner_instruction_index_or_unknown: EventOrderComponent::not_applicable(),
        log_index_or_unknown: EventOrderComponent::not_applicable(),
        event_seq_in_process: 10 + age_ms / 1_000,
        observed_at_wall_ms,
    }
}
