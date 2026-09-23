//! Offline-only bounded replay of an existing, receipt-bound raw Tape V4 prefix.
//! No network clients, wallet access, execution, outcome rows, or FSC gate.
use anyhow::{ensure, Context, Result};
use ghost_brain::config::GatekeeperV2Config;
use ghost_core::pump_research_exact_tape_v2::{
    PumpExactStateRawCodecV2, PumpExactStateRawRecordV2, PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2,
    PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2,
};
use ghost_launcher::events::PoolTransaction;
use ghost_launcher::tx_intelligence::{
    compute_sybil_resistance_with_ftdi_at_cutoff, CpvQueryWindow, CrossPoolVelocityConfig,
    CrossPoolVelocityIndex,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

const MAX_RECORDS: usize = 2048;
const MAX_PREFIX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_POOLS: usize = 128;
const MAX_POOL_TX: usize = 128;
const EARLY_TX_COUNT: usize = 32;

#[derive(Default, Clone)]
struct PoolWindow {
    creator: Option<String>,
    txs: VecDeque<PoolTransaction>,
    dropped: usize,
}

fn insert_pool<'a>(
    pools: &'a mut BTreeMap<String, PoolWindow>,
    pool: &str,
) -> Option<&'a mut PoolWindow> {
    if !pools.contains_key(pool) && pools.len() >= MAX_POOLS {
        return None;
    }
    Some(pools.entry(pool.into()).or_default())
}

fn digest_array(value: &Value) -> Result<String> {
    let array = value.as_array().context("digest is not an array")?;
    ensure!(array.len() == 32, "digest is not 32 bytes");
    let bytes = array
        .iter()
        .map(|v| {
            u8::try_from(v.as_u64().context("invalid digest byte")?).context("digest byte range")
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(bytes.iter().map(|v| format!("{v:02x}")).collect())
}

fn file_digests(path: &Path) -> Result<(String, String, u64)> {
    let file = File::open(path)?;
    ensure!(file.metadata()?.is_file(), "not a regular file");
    let mut input = BufReader::new(file);
    let mut sha = Sha256::new();
    let mut blake = blake3::Hasher::new();
    let mut size = 0u64;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
        blake.update(&buffer[..n]);
        size += n as u64;
    }
    Ok((
        format!("{:x}", sha.finalize()),
        blake.finalize().to_hex().to_string(),
        size,
    ))
}

fn read_frame(reader: &mut impl Read, remaining: u64) -> Result<Option<Vec<u8>>> {
    let mut length = [0u8; 4];
    if reader.read(&mut length[..1])? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..])?;
    let n = u32::from_le_bytes(length) as usize;
    ensure!(
        n <= PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2,
        "raw frame exceeds frozen limit"
    );
    let frame_len = n.checked_add(36).context("frame length overflow")?;
    if frame_len as u64 > remaining {
        return Ok(None);
    }
    let mut frame = vec![0; frame_len];
    frame[..4].copy_from_slice(&length);
    reader.read_exact(&mut frame[4..])?;
    Ok(Some(frame))
}

fn export_checkpoint(
    writer: &mut impl Write,
    label: &str,
    pools: &BTreeMap<String, PoolWindow>,
    cutoff: u64,
    index: &CrossPoolVelocityIndex,
    config: &CrossPoolVelocityConfig,
) -> Result<[usize; 5]> {
    let mut full = [0; 5];
    for (pool, state) in pools {
        let metrics = compute_sybil_resistance_with_ftdi_at_cutoff(
            state.txs.iter(),
            state.creator.as_deref(),
            cutoff,
        );
        let times: Vec<_> = state
            .txs
            .iter()
            .filter_map(|tx| tx.event_time.compat_event_ts_ms(None))
            .collect();
        let cpv = index.compute_for_transactions_at(
            pool,
            state.txs.iter(),
            CpvQueryWindow {
                signer_window_start_ms: times.iter().copied().min().unwrap_or(0),
                anchor_ms: times.iter().copied().max().unwrap_or(0),
                cutoff_received_ms: cutoff,
            },
            config,
        );
        for (i, clean) in [
            metrics.ftdi.has_full_quality(),
            metrics.dbia.has_full_quality(),
            metrics.sfd.has_full_quality(),
            cpv.status == ghost_core::checkpoint::MetricEvidenceQuality::Clean,
            metrics.des.has_full_quality(),
        ]
        .into_iter()
        .enumerate()
        {
            full[i] += usize::from(clean);
        }
        metrics
            .ftdi
            .evidence_v2()
            .validate()
            .map_err(anyhow::Error::msg)?;
        metrics.des.validate().map_err(anyhow::Error::msg)?;
        let encoded = serde_json::to_vec(&metrics.features)?;
        let decoded: ghost_core::tx_intelligence::types::SybilResistanceFeatures =
            serde_json::from_slice(&encoded)?;
        ensure!(
            decoded == metrics.features,
            "measurement JSON roundtrip drift for {pool}"
        );
        let record = json!({"checkpoint": label, "pool": pool, "receiver_cutoff_ms": cutoff,
            "retained_transactions": state.txs.len(), "dropped_transactions": state.dropped,
            "known_creator_from_prefix": state.creator, "measurements": metrics.features,
            "cpv_evidence": cpv.evidence_context()});
        serde_json::to_writer(&mut *writer, &record)?;
        writer.write_all(b"\n")?;
    }
    Ok(full)
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: m6_bounded_replay <raw-v2-directory> <new-output-directory>"
    );
    let raw = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    ensure!(!output.exists(), "output directory already exists");
    let start_bytes = std::fs::read(raw.join("run_start_manifest_v2.json"))?;
    let completion_bytes = std::fs::read(raw.join("run_completion_receipt_v2.json"))?;
    let start: Value = serde_json::from_slice(&start_bytes)?;
    let completion: Value = serde_json::from_slice(&completion_bytes)?;
    ensure!(
        start["run_id"] == completion["run_id"] && completion["status"] == "complete",
        "run receipts differ or incomplete"
    );
    let receipt = &completion["segment_list"][0];
    ensure!(
        receipt["filename"] == "segment_00000.bin" && receipt["segment_index"] == 0,
        "not the first frozen segment"
    );
    let segment = raw.join("segment_00000.bin");
    let hashes = file_digests(&segment)?;
    ensure!(
        hashes.0 == digest_array(&receipt["file_sha256"])?
            && hashes.1 == digest_array(&receipt["file_blake3"])?,
        "segment digest differs from receipt"
    );
    ensure!(
        Some(hashes.2) == receipt["file_bytes"].as_u64(),
        "segment byte count differs"
    );
    std::fs::create_dir(&output)?;
    let mut reader = BufReader::new(File::open(&segment)?);
    let mut header_bytes = vec![0u8; PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2.len()];
    reader.read_exact(&mut header_bytes)?;
    let frame = read_frame(&mut reader, MAX_PREFIX_BYTES)?.context("missing header")?;
    header_bytes.extend(frame);
    let header = PumpExactStateRawCodecV2::decode_segment_header(&header_bytes)?;
    ensure!(
        header.segment_index == 0 && header.previous_segment_blake3.is_none(),
        "not an initial segment"
    );
    ensure!(
        Some(header.run_id.as_str()) == start["run_id"].as_str(),
        "header run id mismatch"
    );
    let header_contract: String = header
        .capture_contract_sha256
        .into_inner()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect();
    ensure!(
        Some(header_contract.as_str()) == start["capture_contract_sha256"].as_str(),
        "header capture contract mismatch"
    );
    let mut offset = header_bytes.len() as u64;
    let mut prefix_hash = blake3::Hasher::new();
    prefix_hash.update(&header_bytes);
    let mut gatekeeper = GatekeeperV2Config::default();
    gatekeeper.cpv_lookback_window_s = 1;
    gatekeeper.cpv_global_signer_cap = 4096;
    gatekeeper.cpv_per_signer_cap = 32;
    let cpv_config = CrossPoolVelocityConfig::from_gatekeeper_config(&gatekeeper);
    let index = CrossPoolVelocityIndex::new();
    let parser = seer::binary_parser::BinaryParser::new(false);
    let mut pools = BTreeMap::<String, PoolWindow>::new();
    let mut records = 0usize;
    let mut tx_records = 0usize;
    let mut trades = 0usize;
    let mut source_markers = 0usize;
    let mut gaps = 0usize;
    let mut last_receiver = 0u64;
    let mut first_receiver = None;
    let mut early = None;
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join("measurements.jsonl"))?;
    let mut writer = BufWriter::new(file);
    while records < MAX_RECORDS {
        let Some(frame) = read_frame(&mut reader, MAX_PREFIX_BYTES.saturating_sub(offset))? else {
            break;
        };
        let record = PumpExactStateRawCodecV2::decode_record(&frame)?;
        offset += frame.len() as u64;
        prefix_hash.update(&frame);
        records += 1;
        match record {
            PumpExactStateRawRecordV2::PrimaryTransaction(tx) => {
                ensure!(
                    tx.source.stream_epoch == header.stream_epoch,
                    "stream epoch drift"
                );
                let received = tx
                    .event_time
                    .ingress_wall_ts_ms
                    .context("missing captured receiver time")?;
                first_receiver.get_or_insert(received);
                last_receiver = last_receiver.max(received);
                let event = seer::grpc_connection::decode_research_raw_transaction_v2(&tx)?;
                let success = matches!(&event, seer::types::GeyserEvent::Transaction { success: true, metadata_availability, .. } if metadata_availability.status_known);
                let bundle = parser.parse_transaction_bundle(&event)?;
                if success {
                    if let Some(init) = bundle.initialize_pool {
                        if let Some(state) = insert_pool(&mut pools, &init.pool_amm_id.to_string())
                        {
                            state.creator = Some(init.creator.to_string());
                        }
                    }
                }
                for trade in bundle.trades {
                    let trade =
                        ghost_launcher::components::seer::trade_event_to_pool_transaction(&trade);
                    index.observe_transaction_at(&trade.pool_amm_id, &trade, received, &cpv_config);
                    if let Some(state) = insert_pool(&mut pools, &trade.pool_amm_id) {
                        if state.txs.len() == MAX_POOL_TX {
                            state.txs.pop_front();
                            state.dropped += 1;
                        }
                        state.txs.push_back(trade);
                    }
                    trades += 1;
                }
                tx_records += 1;
                if tx_records == EARLY_TX_COUNT {
                    export_checkpoint(
                        &mut writer,
                        "early",
                        &pools,
                        last_receiver,
                        &index,
                        &cpv_config,
                    )?;
                    early = Some((last_receiver, pools.clone()));
                }
            }
            PumpExactStateRawRecordV2::PrimaryBlockMeta(meta) => {
                ensure!(
                    meta.source.stream_epoch == header.stream_epoch,
                    "progress epoch drift"
                );
                ensure!(
                    *blake3::hash(&meta.source_payload).as_bytes()
                        == meta.source.payload_hash_blake3.into_inner(),
                    "BlockMeta payload hash mismatch"
                );
                let received = meta
                    .event_time
                    .ingress_wall_ts_ms
                    .context("missing BlockMeta receiver time")?;
                first_receiver.get_or_insert(received);
                last_receiver = last_receiver.max(received);
                index.observe_source_progress(
                    meta.source.stream_epoch,
                    received,
                    received,
                    last_receiver,
                    &cpv_config,
                );
                source_markers += 1;
            }
            PumpExactStateRawRecordV2::CoverageGap(gap) => {
                index.mark_stream_gap(gap.started_at_wall_ms);
                gaps += 1;
            }
            PumpExactStateRawRecordV2::SegmentClosed(_) => break,
            _ => {}
        }
    }
    ensure!(
        tx_records > 0 && trades > 0,
        "bounded prefix has no parsed trades"
    );
    let full = export_checkpoint(
        &mut writer,
        "final",
        &pools,
        last_receiver,
        &index,
        &cpv_config,
    )?;
    writer.flush()?;
    let mut cutoff_checks = 0usize;
    if let Some((cutoff, early_pools)) = early {
        for (pool, state) in early_pools {
            let expected = compute_sybil_resistance_with_ftdi_at_cutoff(
                state.txs.iter(),
                state.creator.as_deref(),
                cutoff,
            );
            let mut with_future = state.txs;
            if let Some(later) = pools.get(&pool) {
                with_future.extend(
                    later
                        .txs
                        .iter()
                        .filter(|tx| tx.event_time.ingress_wall_ts_ms.is_some_and(|t| t > cutoff))
                        .cloned(),
                );
            }
            let actual = compute_sybil_resistance_with_ftdi_at_cutoff(
                with_future.iter(),
                state.creator.as_deref(),
                cutoff,
            );
            ensure!(
                actual == expected,
                "future input altered the earlier cutoff for {pool}"
            );
            cutoff_checks += 1;
        }
    }
    ensure!(
        file_digests(&segment)? == hashes,
        "source changed during replay"
    );
    let summary = json!({"scope":"bounded_offline_metric_replay_not_strategy_backtest",
        "run_id":header.run_id,"segment_sha256":hashes.0,"segment_blake3":hashes.1,
        "start_manifest_sha256":format!("{:x}",Sha256::digest(&start_bytes)),
        "completion_receipt_sha256":format!("{:x}",Sha256::digest(&completion_bytes)),
        "prefix_blake3":prefix_hash.finalize().to_hex().to_string(),"prefix_end_offset":offset,
        "record_limit":MAX_RECORDS,"byte_limit":MAX_PREFIX_BYTES,"records":records,
        "source_transaction_records":tx_records,"parsed_trades":trades,"source_progress_markers":source_markers,"gaps":gaps,
        "first_receiver_ms":first_receiver,"final_receiver_cutoff_ms":last_receiver,
        "retained_pools":pools.len(),"pool_cap":MAX_POOLS,"per_pool_tx_cap":MAX_POOL_TX,
        "max_retained_transactions":pools.values().map(|s|s.txs.len()).max().unwrap_or(0),
        "total_evicted_transactions":pools.values().map(|s|s.dropped).sum::<usize>(),
        "clean_measurements_by_metric":{"FTDI":full[0],"DBIA":full[1],"SFD":full[2],"CPV":full[3],"DES":full[4]},
        "earlier_cutoff_checks":cutoff_checks,"fsc_used_as_gate":false,"network_used":false});
    let mut out = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join("summary.json"))?;
    serde_json::to_writer_pretty(&mut out, &summary)?;
    out.write_all(b"\n")?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
