//! Read-only live Gate 0. Starts Seer and existing session calculators only.
use anyhow::{ensure, Context, Result};
use clap::Parser;
use ghost_launcher::gate0::{Gate0, Gate0Config, WSOL};
use seer::{
    config::{
        ConnectionMode, FundingLaneMode, SeerConfig, SeerSourceMode, StreamMode, TxFilterStrategy,
    },
    ipc::{
        create_ipc_channel, IpcMetrics, IpcReceiver, IpcSender, LocalCoverageGapStateV1, SeerEvent,
    },
    Seer,
};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::BufWriter,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, default_value = "GHO_GATE0_GRPC_ENDPOINT")]
    endpoint_env: String,
    #[arg(long, default_value = "GHO_GATE0_GRPC_TOKEN")]
    token_env: String,
    #[arg(long, default_value = "x-token")]
    auth_header: String,
    #[arg(long)]
    no_funding: bool,
    /// Check the actual source, then stop with censored open observations.
    #[arg(long)]
    smoke_seconds: Option<u64>,
}

// Jedna skala epoch dla cutoffów i raw ingress. Obcięcie epoch i elapsed
// osobno zaniżało czas konsumenta o 1 ms; CPV uznawało to za utratę źródła.
fn consumer_epoch_ms(epoch: u64, elapsed_ms: u64, wall_ms: u64, previous: u64) -> u64 {
    previous.max(epoch.saturating_add(elapsed_ms)).max(wall_ms)
}

fn event_detected_at(event: &SeerEvent) -> SystemTime {
    match event {
        SeerEvent::PrimaryTradeFeedProgress(event) => event.detected_at,
        SeerEvent::PoolDetected(event) => event.detected_at,
        SeerEvent::Trade(event) => event.detected_at,
        SeerEvent::FundingTransfer(event) => event.detected_at,
        SeerEvent::AccountUpdate(event) => event.detected_at,
        SeerEvent::ExecutionAccountEvidence(event) => event.detected_at,
    }
}

fn local_gap_reason(state: &LocalCoverageGapStateV1) -> Option<String> {
    if state.overflowed {
        return Some("local_coverage_gap_control_overflow".to_string());
    }
    if state.notices.is_empty() {
        return None;
    }
    Some(format!(
        "local_coverage_gap:{}",
        state
            .notices
            .iter()
            .map(|notice| format!("{}:{}", notice.provider_id, notice.reason.as_str()))
            .collect::<Vec<_>>()
            .join(",")
    ))
}

#[derive(Default)]
struct LoadTelemetry {
    samples: u64,
    events_consumed: u64,
    max_primary_ingress_depth: usize,
    max_primary_ingress_high_water: usize,
    max_funding_ingress_depth: usize,
    max_funding_ingress_high_water: usize,
    max_ipc_egress_depth: usize,
    max_ipc_downstream_depth: usize,
    max_consumer_lag_ms: u64,
}

impl LoadTelemetry {
    fn observe_queues(&mut self, seer: &Seer, ipc: &IpcSender, rx: &IpcReceiver) {
        let snapshot = seer.gate0_queue_snapshot();
        self.samples = self.samples.saturating_add(1);
        if let Some(primary) = snapshot.primary {
            self.max_primary_ingress_depth = self.max_primary_ingress_depth.max(primary.depth);
            self.max_primary_ingress_high_water =
                self.max_primary_ingress_high_water.max(primary.high_water);
        }
        if let Some(funding) = snapshot.funding {
            self.max_funding_ingress_depth = self.max_funding_ingress_depth.max(funding.depth);
            self.max_funding_ingress_high_water =
                self.max_funding_ingress_high_water.max(funding.high_water);
        }
        self.max_ipc_egress_depth = self.max_ipc_egress_depth.max(ipc.current_queue_length());
        self.max_ipc_downstream_depth = self.max_ipc_downstream_depth.max(rx.pending_len());
    }

    fn observe_event(&mut self, event: &SeerEvent) {
        self.events_consumed = self.events_consumed.saturating_add(1);
        if let Ok(age) = event_detected_at(event).elapsed() {
            self.max_consumer_lag_ms = self
                .max_consumer_lag_ms
                .max(u64::try_from(age.as_millis()).unwrap_or(u64::MAX));
        }
    }

    fn snapshot(
        &self,
        seer: &Seer,
        ipc: &IpcSender,
        rx: &IpcReceiver,
        ipc_metrics: &IpcMetrics,
    ) -> Value {
        let queues = seer.gate0_queue_snapshot();
        let ingress_json = |queue: Option<seer::grpc_connection::IngressQueueSnapshot>| {
            queue.map(|queue| {
                json!({
                    "current": queue.depth,
                    "capacity": queue.capacity,
                    "high_water": queue.high_water,
                    "received": queue.received,
                    "overflow_dropped": queue.overflow_dropped
                })
            })
        };
        json!({
            "samples": self.samples,
            "events_consumed": self.events_consumed,
            "max_consumer_lag_ms": self.max_consumer_lag_ms,
            "pending_mapping": {
                "buffered_total": queues.pending_mapping_buffered,
                "expired_total": queues.pending_trade_expired
            },
            "primary_ingress": ingress_json(queues.primary),
            "funding_ingress": ingress_json(queues.funding),
            "ipc_egress": {
                "current": ipc.current_queue_length(),
                "capacity": ipc.queue_capacity(),
                "sampled_high_water": self.max_ipc_egress_depth,
                "reported_high_water": ipc_metrics.queue_length_max.get()
            },
            "ipc_downstream": {
                "current": rx.pending_len(),
                "capacity": rx.pending_capacity(),
                "sampled_high_water": self.max_ipc_downstream_depth
            },
            "sampled_max": {
                "primary_ingress_depth": self.max_primary_ingress_depth,
                "primary_ingress_high_water": self.max_primary_ingress_high_water,
                "funding_ingress_depth": self.max_funding_ingress_depth,
                "funding_ingress_high_water": self.max_funding_ingress_high_water
            }
        })
    }
}

#[tokio::main(worker_threads = 8)]
async fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        // Zachowaj dowody pracy transportu także przed zadziałaniem guardu.
        .with_env_filter("warn,seer::grpc_connection=info")
        .with_writer(std::io::stderr)
        .init();
    let config: Gate0Config = match args.config {
        Some(path) => toml::from_str(&std::fs::read_to_string(path)?)?,
        None => Default::default(),
    };
    config.validate()?;
    let endpoint = std::env::var(&args.endpoint_env)
        .context("set the requested gRPC endpoint environment variable")?;
    let url = reqwest::Url::parse(&endpoint).context("invalid gRPC endpoint")?;
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.query().is_none(),
        "credentials must be supplied in metadata, not the endpoint URL"
    );
    let mut source = SeerConfig {
        connection_mode: ConnectionMode::Grpc,
        source_mode: Some(SeerSourceMode::GeyserGrpc),
        commitment: config.source_commitment.clone(),
        grpc_endpoint: endpoint,
        grpc_auth_token: std::env::var(&args.token_env).ok(),
        grpc_auth_header: args.auth_header,
        grpc_manual_backfill_enabled: false,
        grpc_commitment_fallback_to_websocket: false,
        rpc_endpoint: std::env::var("GHO_GATE0_RPC_ENDPOINT")
            .unwrap_or_else(|_| "http://127.0.0.1:9".into()),
        stream_mode: StreamMode::SingleGlobal,
        tx_filter_strategy: TxFilterStrategy::All,
        funding_lane_mode: if args.no_funding {
            FundingLaneMode::Disabled
        } else {
            FundingLaneMode::FullChain
        },
        canonical_account_update_relay_enabled: true,
        watched_pools_ttl_ms: 660_000,
        watched_pools_cap: 16_384,
        metrics_port: 0,
        // Bounded burst capacity for primary and full-chain funding. Loss still
        // invalidates the run instead of silently dropping observations.
        ingress_queue_capacity: 16_384,
        ipc_config: seer::ipc::IpcChannelConfig {
            buffer_size: 100_000,
            ..Default::default()
        },
        filter: seer::config::FilterConfig {
            enable_pumpfun: true,
            enable_bonkfun: false,
            allowed_quote_mints: vec![WSOL.to_string()],
            min_initial_liquidity_sol: None,
        },
        ..SeerConfig::default()
    };
    source.program_streams.enabled = false;
    let (tx, mut rx, ipc_metrics) = create_ipc_channel(source.ipc_config.clone());
    let ipc_probe = tx.clone();
    let mut gaps = rx.local_coverage_gap_receiver();
    let seer = Arc::new(Seer::new_with_ipc(source, tx).with_gate0_observation());
    let (funding_tx, mut funding_rx) = tokio::sync::watch::channel(false);
    seer.set_authoritative_funding_stream_availability_sender(funding_tx);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.output)?;
    let epoch = seer::types::ingress_epoch_ms();
    let clock = Instant::now();
    let mut previous_now = epoch;
    let mut now = || {
        previous_now = consumer_epoch_ms(
            epoch,
            u64::try_from(clock.elapsed().as_millis()).unwrap_or(u64::MAX),
            seer::types::ingress_epoch_ms(),
            previous_now,
        );
        previous_now
    };
    let mut gate = Gate0::new(
        config,
        format!("gate0-{epoch}"),
        epoch,
        BufWriter::new(file),
    )?;
    let mut source_task = tokio::spawn(Arc::clone(&seer).run());
    let mut timer = tokio::time::interval(Duration::from_millis(100));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_progress = epoch;
    let mut progress_count = 0u64;
    let mut trades = 0u64;
    let mut amm_states = 0u64;
    let mut heartbeat = epoch;
    let telemetry_interval_ms = if args.smoke_seconds.is_some() {
        1_000
    } else {
        60_000
    };
    let mut end_reason: Option<String> = None;
    let mut shutdown_errors = Vec::new();
    let mut funding_open = true;
    let mut progress_key = None;
    let mut load = LoadTelemetry::default();
    let result:Result<()>=async {
        loop {
            tokio::select! {
                _=tokio::signal::ctrl_c()=>{end_reason=Some("interrupted".to_string());break;}
                changed=gaps.changed()=>{
                    changed.context("IPC gap channel closed")?;
                    if let Some(reason)=local_gap_reason(&gaps.borrow()) {
                        anyhow::bail!(reason);
                    }
                }
                changed=funding_rx.changed(), if funding_open=>{ if changed.is_ok(){gate.set_funding_available(*funding_rx.borrow_and_update());} else {funding_open=false;gate.set_funding_available(false);} }
                event=rx.recv()=>{
                    let event=event.context("Seer IPC closed")?; let at=now();
                    load.observe_event(&event);
                    match event {
                        SeerEvent::PoolDetected(e)=>{
                            let admitted_before=gate.summary.admitted;
                            let pool=e.candidate.pool_amm_id;
                            let mint=e.candidate.base_mint;
                            gate.on_pool(&e,at)?;
                            if gate.summary.admitted>admitted_before {
                                seer.gate0_register_cohort(pool,mint);
                            }
                        },
                        SeerEvent::Trade(e)=>{trades+=1;amm_states+=u64::from(e.trade.amm_observation.is_some());gate.on_trade(&e.trade,at)?;}
                        SeerEvent::FundingTransfer(e)=>gate.on_funding(&e,at)?,
                        SeerEvent::AccountUpdate(e)=>gate.on_account(&e,at)?,
                        SeerEvent::PrimaryTradeFeedProgress(e)=>{
                            gate.on_progress(&e.progress,at)?;
                            if !e.progress.gap {
                                let key=(e.progress.epoch,e.progress.event_ms);
                                if progress_key.is_none_or(|old|key>old) {last_progress=at;progress_count+=1;progress_key=Some(key);}
                            }
                        }
                        SeerEvent::ExecutionAccountEvidence(_)=>{},
                    }
                }
                _=timer.tick()=>{
                    let at=now(); ensure!(!source_task.is_finished(),"seer_task_ended");
                    ensure!(at.saturating_sub(last_progress)<if progress_count==0{30_000}else{10_000},"primary progress unavailable/stale");
                    gate.tick(at)?;
                    load.observe_queues(&seer,&ipc_probe,&rx);
                    let integrity=seer.gate0_queue_snapshot();
                    ensure!(integrity.pending_trade_expired == 0,
                        "pending mapping trade expired before authoritative mapping/replay");
                    if at.saturating_sub(heartbeat)>=telemetry_interval_ms {
                        let queues=load.snapshot(&seer,&ipc_probe,&rx,&ipc_metrics);
                        eprintln!("Gate0 load={} admitted={} active={} phases={:?} gems={} trades={} amm_states={}",queues,gate.summary.admitted,gate.active_tokens(),gate.summary.phase_counts,gate.summary.gems,trades,amm_states);
                        heartbeat=at;
                    }
                    if let Some(smoke_seconds) = args.smoke_seconds.filter(|s| at.saturating_sub(epoch) >= s.saturating_mul(1_000)) {
                        ensure!(progress_count > 0 && trades > 0 && gate.summary.admitted > 0,
                            "source smoke lacks progress/create/trade evidence");
                        if smoke_seconds >= 600 {
                            ensure!(gate.summary.phase_counts[4] > 0,
                                "lifecycle smoke has no phase-V snapshot");
                            ensure!(gate.summary.terminal_counts.get("completed").copied().unwrap_or(0) > 0,
                                "lifecycle smoke has no completed 600s terminal");
                            ensure!(
                                gate.summary.completed_with_migration_initial_state > 0,
                                "lifecycle smoke has no completed token with tracked PumpSwap initial state"
                            );
                        }
                        end_reason=Some("smoke_only".to_string());break;
                    }
                    if gate.drained(at) {break;}
                }
            }
        }
        Ok(())
    }.await;
    let primary_error = result.as_ref().err().map(ToString::to_string);
    if end_reason.is_none() {
        end_reason = primary_error.clone();
    }

    seer.request_shutdown();
    let deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(deadline);
    let mut source_task_error = None;
    loop {
        tokio::select! {
            joined=&mut source_task=>{
                source_task_error=Some(match joined {
                    Ok(Ok(()))=>"seer_task_ended_without_error".to_string(),
                    Ok(Err(error))=>format!("seer_task:{error}"),
                    Err(error)=>format!("seer_task_join:{error}"),
                });
                break;
            },
            _=&mut deadline=>{
                source_task.abort();
                let _=source_task.await;
                shutdown_errors.push("source_shutdown_timeout".to_string());
                break;
            },
            _=rx.recv()=>{},
        }
    }
    if primary_error.as_deref() == Some("seer_task_ended") {
        end_reason = source_task_error.clone().or(primary_error.clone());
    } else if let Some(error) = source_task_error {
        if error != "seer_task_ended_without_error" {
            shutdown_errors.push(error);
        }
    }

    let shutdown = seer.shutdown_dispatchers();
    let dispatcher_deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(shutdown, dispatcher_deadline);
    loop {
        tokio::select! {
            result=&mut shutdown=>{
                if let Err(error)=result {
                    shutdown_errors.push(format!("dispatcher_shutdown:{error}"));
                }
                break;
            },
            _=&mut dispatcher_deadline=>{
                shutdown_errors.push("dispatcher_shutdown_timeout".to_string());
                break;
            },
            _=rx.recv()=>{},
        }
    }

    let shutdown_error = (!shutdown_errors.is_empty()).then(|| shutdown_errors.join("; "));
    if end_reason.is_none() && shutdown_error.is_some() {
        end_reason = Some("shutdown_error".to_string());
    }
    load.observe_queues(&seer, &ipc_probe, &rx);
    let runtime_diagnostics = load.snapshot(&seer, &ipc_probe, &rx, &ipc_metrics);
    let writer = gate.close(
        now(),
        end_reason.as_deref(),
        shutdown_error.as_deref(),
        runtime_diagnostics,
    )?;
    writer.into_inner()?.sync_all()?;

    result?;
    if let Some(error) = shutdown_error {
        anyhow::bail!("source shutdown failed: {error}");
    }
    println!(
        "Gate0 finished: progress={progress_count}, trades={trades}, amm_states={amm_states}, reason={end_reason:?}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::LocalCoverageGapReasonV1;
    use seer::ipc::LocalCoverageGapNoticeV1;

    #[test]
    fn consumer_clock_cannot_precede_observed_ingress_or_regress() {
        assert_eq!(consumer_epoch_ms(1_000, 1, 1_002, 1_000), 1_002);
        assert_eq!(consumer_epoch_ms(1_000, 2, 998, 1_002), 1_002);
        assert_eq!(consumer_epoch_ms(1_000, 4, 999, 1_002), 1_004);
    }

    #[test]
    fn local_gap_reason_preserves_provider_and_ingress_root_cause() {
        let state = LocalCoverageGapStateV1 {
            notices: vec![LocalCoverageGapNoticeV1 {
                provider_id: "primary".to_string(),
                reason: LocalCoverageGapReasonV1::IngressQueueSaturated,
            }],
            ..Default::default()
        };
        assert_eq!(
            local_gap_reason(&state).as_deref(),
            Some("local_coverage_gap:primary:ingress_queue_saturated")
        );
    }

    #[test]
    fn local_gap_control_overflow_is_a_distinct_root_cause() {
        let state = LocalCoverageGapStateV1 {
            overflowed: true,
            ..Default::default()
        };
        assert_eq!(
            local_gap_reason(&state).as_deref(),
            Some("local_coverage_gap_control_overflow")
        );
    }
}
