# Polecenia runtime Ghost / Selector

Ten plik jest praktycznym runbookiem dla aktualnego runtime Ghost w `/root/Gho`.
Zakłada shadow-only workflow, Guard/Lifecycle launcher, Gatekeeper decision logs,
BUY shadow lifecycle oraz osobną ścieżkę counterfactual probe dla decyzji innych
niż BUY.

Zasady:

- nie uruchamiaj dwóch runów na tym samym `scope`;
- przed długim runem zawsze wykonaj dry-run/preflight;
- po starcie obserwuj pierwsze 10-15 minut;
- nie traktuj `shadow_lifecycle.jsonl` jako pełnej populacji Gatekeepera;
- dla pełnej populacji decyzji używaj decision logs plus probe artifacts;
- nie kasuj `logs/`, `datasets/`, `reports/` bez manifestu i planu cleanupu.

## 1. Zmienne robocze dla nowego runu

Przykład dla nowego R29:

```bash
cd /root/Gho

export RUN_ID="r29"
export RUNTIME_SCOPE="shadow-burnin-v3-r29-all-decision-counterfactual-30-30"
export TMUX_SESSION="r29-all-decision"
export RUNTIME_CONFIG="configs/rollout/${RUNTIME_SCOPE}.toml"
export BRAIN_CONFIG="configs/rollout/ghost_brain_selector_dataset_sampler_${RUN_ID}.toml"
export REPORT_DIR="reports/selector/${RUNTIME_SCOPE}/run_lifecycle_guard_$(date -u +%Y%m%dT%H%M%SZ)"
export SELECTOR_SCOPE="selector-phase1-pumpfun-sol-v1-$(date -u +%Y%m%d)-${RUN_ID}-all-decision-final"
```

## 2. Przygotowanie nowego folderu i configów

Najbezpieczniejszy tryb to skopiować ostatni poprawny rollout config i brain
config, a potem zmienić tylko scope, ścieżki i jawnie uzgodnione parametry.

Przykład na bazie R28:

```bash
cd /root/Gho

cp configs/rollout/shadow-burnin-v3-r28-all-decision-counterfactual-30-30-maxwait4000.toml "$RUNTIME_CONFIG"
cp configs/rollout/ghost_brain_selector_dataset_sampler_r28_maxwait4000.toml "$BRAIN_CONFIG"

perl -0pi -e "s/shadow-burnin-v3-r28-all-decision-counterfactual-30-30-maxwait4000/${RUNTIME_SCOPE}/g" "$RUNTIME_CONFIG"
perl -0pi -e "s#ghost_brain_selector_dataset_sampler_r28_maxwait4000.toml#$(basename "$BRAIN_CONFIG")#g" "$RUNTIME_CONFIG"
```

Jeśli zmieniasz `max_wait_time_ms`, sprawdź spójność DOW:

```bash
rg -n "max_wait_time_ms|early_entry_max_ms|normal_window_ms|extended_window_ms" "$BRAIN_CONFIG"
```

Kontrakt: `[gatekeeper_v2.dow].extended_window_ms` nie może być większe niż
`[gatekeeper_v2].max_wait_time_ms`, bo preflight powinien to odrzucić.

## 3. Szybka inspekcja configu

```bash
rg -n "ghost_brain_config_path|funding_lane_mode|source_mode|stream_mode|p37_shadow_probe|include_verdict_types|sample_modulus|sample_threshold|exclude_active_buy_rows|lifecycle_log_path|entry_log_path" "$RUNTIME_CONFIG"

rg -n "max_wait_time_ms|early_entry_max_ms|normal_window_ms|extended_window_ms|target|stop|horizon|fsc|funding_lookback" "$BRAIN_CONFIG"
```

Oczekiwane dla runów all-decision/counterfactual:

```toml
[p37_shadow_probe]
enabled = true
include_verdict_types = ["BUY", "REJECT", "TIMEOUT", "PENDING"]
sample_modulus = 1
sample_threshold = 1
exclude_active_buy_rows = false
```

Oczekiwane dla FSC full-chain:

```toml
[seer]
source_mode = "grpc"
stream_mode = "single_global"
funding_lane_mode = "full_chain"

[seer.program_streams]
enabled = true
max_streams = 2
enabled_topics = [
  "solana.pump_fun.buy",
  "solana.pump_fun.buy_exact_sol_in",
]
```

## 4. Minimalny preflight systemowy

```bash
df -h /root/Gho
tmux ls || true
pgrep -af "ghost-launcher|cargo|python3|${RUNTIME_SCOPE}|${TMUX_SESSION}" || true
```

Jeśli jest aktywny poprzedni run, nie startuj nowego bez świadomej decyzji.

## 5. Static preflight bez startowania runu

```bash
python3 scripts/start_selector_lifecycle_run.py \
  --root /root/Gho \
  --scope "$RUNTIME_SCOPE" \
  --config "$RUNTIME_CONFIG" \
  --tmux-session "$TMUX_SESSION" \
  --output-dir "$REPORT_DIR" \
  --min-free-gb 20 \
  --event-canary-seconds 60 \
  --lifecycle-proof-timeout-seconds 900 \
  --lifecycle-poll-seconds 30 \
  --min-reporter-rows 1 \
  --dry-run
```

PASS powinien zakończyć się komunikatem podobnym do:

```text
SELECTOR_LIFECYCLE_RUN_STATIC_PREFLIGHT_PASS
status=PASS report=...
```

## 6. Start runu przez lifecycle guard launcher

```bash
python3 scripts/start_selector_lifecycle_run.py \
  --root /root/Gho \
  --scope "$RUNTIME_SCOPE" \
  --config "$RUNTIME_CONFIG" \
  --tmux-session "$TMUX_SESSION" \
  --output-dir "$REPORT_DIR" \
  --min-free-gb 20 \
  --event-canary-seconds 60 \
  --lifecycle-proof-timeout-seconds 900 \
  --lifecycle-poll-seconds 30 \
  --min-reporter-rows 1
```

PASS powinien zakończyć się komunikatem:

```text
SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF
status=PASS report=...
```

Jeśli trzeba zbudować release przed startem:

```bash
python3 scripts/start_selector_lifecycle_run.py \
  --root /root/Gho \
  --scope "$RUNTIME_SCOPE" \
  --config "$RUNTIME_CONFIG" \
  --tmux-session "$TMUX_SESSION" \
  --output-dir "$REPORT_DIR" \
  --min-free-gb 20 \
  --event-canary-seconds 60 \
  --lifecycle-proof-timeout-seconds 900 \
  --lifecycle-poll-seconds 30 \
  --min-reporter-rows 1 \
  --build-release-before-start
```

## 7. Monitoring procesu i podstawowy health

```bash
tmux ls || true
pgrep -af "ghost-launcher|${RUNTIME_SCOPE}|${TMUX_SESSION}" || true
df -h /root/Gho
```

Podejrzenie tmux bez przejmowania procesu:

```bash
tmux capture-pane -pt "$TMUX_SESSION" -S -80
```

Log launchera:

```bash
tail -n 120 "$REPORT_DIR/runtime.log"
```

Log systemowy runtime:

```bash
tail -n 120 "logs/rollout/${RUNTIME_SCOPE}/system.log.$(date -u +%Y-%m-%d)"
```

## 8. Monitoring ingestu i FSC lane

```bash
rg -n "grpc_global_stream|grpc_funding_lane_full_chain|SUBSCRIBE_SENT|PROGRAM_STREAM|ResourceExhausted|stream limit|limit exceeded|reconnect|WATCHDOG" \
  "logs/rollout/${RUNTIME_SCOPE}/system.log.$(date -u +%Y-%m-%d)" | tail -n 160
```

Oczekiwany ingest shape:

- `grpc_global_stream`
- `grpc_funding_lane_full_chain`
- `solana.pump_fun.buy`
- `solana.pump_fun.buy_exact_sol_in`

Kill conditions:

- `ResourceExhausted`
- `stream limit exceeded`
- reconnect storm
- primary `grpc_global_stream` degraded
- decision rows przestają powstawać
- free disk spada poniżej ustalonego progu bezpieczeństwa

## 9. Liczniki artefaktów shadow/probe

```bash
find "logs/shadow_run/${RUNTIME_SCOPE}" -maxdepth 1 -type f \
  -printf '%f %s %TY-%Tm-%TdT%TH:%TM:%TS\n' | sort
```

Szybkie liczniki linii:

```bash
for f in \
  probe_selection.jsonl \
  probe_transport.jsonl \
  probe_shadow_entries.jsonl \
  probe_shadow_lifecycle.jsonl \
  probe_skips.jsonl \
  shadow_entries.jsonl \
  shadow_lifecycle.jsonl
do
  p="logs/shadow_run/${RUNTIME_SCOPE}/$f"
  [ -f "$p" ] && printf '%-32s %s\n' "$f" "$(wc -l < "$p")"
done
```

## 10. Coverage BUY shadow i non-BUY counterfactual probe

Helper:

```bash
python3 scripts/runtime_probe_coverage.py \
  --root /root/Gho \
  --scope "$RUNTIME_SCOPE"
```

JSON:

```bash
python3 scripts/runtime_probe_coverage.py \
  --root /root/Gho \
  --scope "$RUNTIME_SCOPE" \
  --json > "reports/selector/${RUNTIME_SCOPE}/runtime_probe_coverage_$(date -u +%Y%m%dT%H%M%SZ).json"
```

Najważniejsze pola:

- `probe.selected_candidates`
- `probe.simulated_transport_candidates`
- `probe.lifecycle_candidates`
- `probe.terminal_closed_candidates`
- `probe.simulated_vs_selected`
- `probe.lifecycle_vs_selected`
- `probe.close_reasons`
- `buy.entry_candidates`
- `buy.terminal_closed_candidates`
- `buy.close_reasons`

Interpretacja:

- `shadow_lifecycle.jsonl` = BUY lifecycle;
- `probe_shadow_lifecycle.jsonl` = counterfactual lifecycle dla wybranych decyzji nie-BUY/probe;
- `probe_skips.jsonl` trzeba czytać razem z probe coverage, bo tłumaczy braki w symulacji.

## 11. Coverage FSC z decision logs

Helper:

```bash
python3 scripts/runtime_fsc_coverage.py \
  --root /root/Gho \
  --scope "$RUNTIME_SCOPE"
```

JSON:

```bash
python3 scripts/runtime_fsc_coverage.py \
  --root /root/Gho \
  --scope "$RUNTIME_SCOPE" \
  --json > "reports/selector/${RUNTIME_SCOPE}/runtime_fsc_coverage_$(date -u +%Y%m%dT%H%M%SZ).json"
```

Najważniejsze pola:

- `funding_status`: `clean`, `degraded`, `unavailable`, `missing`;
- `shadow_fsc_reason`;
- `shadow_fsc_policy_signal`;
- `known_source_count`;
- `unknown_buyer_count`;
- `miss_reasons`.

Praktyczna interpretacja:

- `clean` = użyteczna materializacja FSC dla rowa;
- `degraded` = FSC lane działa, ale brakuje wystarczającego przypisania funding source;
- `unavailable` = brak usable FSC dla rowa;
- `FSC_NO_RETAINED_RECIPIENT_HISTORY` oznacza najczęściej brak historii transferów dla buyer wallet w retained index.

## 12. Decision logs: liczba decyzji i verdict family

```bash
python3 - <<'PY'
from pathlib import Path
import json, collections, os
scope=os.environ["RUNTIME_SCOPE"]
base=Path("/root/Gho/logs/rollout")/scope/"decisions"/scope
for path in sorted(base.glob("**/gatekeeper_v2_decisions.jsonl")):
    n=0
    family=collections.Counter()
    verdicts=collections.Counter()
    reasons=collections.Counter()
    for line in path.open(errors="ignore"):
        if not line.strip():
            continue
        n+=1
        row=json.loads(line)
        verdict=str(row.get("verdict_type") or row.get("decision_verdict") or row.get("verdict") or "")
        reason=str(row.get("decision_reason") or row.get("reason_code") or row.get("reason") or "")
        text=(verdict+" "+reason).upper()
        if "BUY" in text and "REJECT" not in text:
            family["BUY"]+=1
        elif "TIMEOUT" in text:
            family["TIMEOUT"]+=1
        elif "REJECT" in text or "HARD_FAIL" in text or "INSUFFICIENT" in text:
            family["REJECT"]+=1
        else:
            family["OTHER"]+=1
        verdicts[verdict]+=1
        reasons[reason]+=1
    print(path)
    print("  rows", n)
    print("  family", family.most_common())
    print("  verdicts", verdicts.most_common(12))
    print("  reasons", reasons.most_common(12))
PY
```

## 13. Zamykanie runu w obecnym stanie

Najpierw sprawdź proces:

```bash
tmux ls || true
pgrep -af "ghost-launcher|${RUNTIME_SCOPE}|${TMUX_SESSION}" || true
```

Zatrzymanie przez tmux:

```bash
tmux send-keys -t "$TMUX_SESSION" C-c
sleep 10
pgrep -af "ghost-launcher|${RUNTIME_SCOPE}|${TMUX_SESSION}" || true
```

Jeśli proces nie kończy się po normalnym `C-c`, dopiero wtedy podejmij świadomą
decyzję o mocniejszym zatrzymaniu. Nie używaj `kill -9` jako pierwszej opcji.

## 14. Budowa candidate universe z decyzji Gatekeepera

Znajdź decision log:

```bash
find "logs/rollout/${RUNTIME_SCOPE}/decisions/${RUNTIME_SCOPE}" -name gatekeeper_v2_decisions.jsonl -print
```

Zbuduj universe z decyzji, jeśli nie ma pełnego event universe:

```bash
mkdir -p "datasets/selector/${SELECTOR_SCOPE}"

python3 scripts/build_selector_candidate_universe.py \
  --decisions "logs/rollout/${RUNTIME_SCOPE}/decisions/${RUNTIME_SCOPE}/v2.5/v25_shadow/<HASH>/gatekeeper_v2_decisions.jsonl" \
  --output "datasets/selector/${SELECTOR_SCOPE}/candidate_universe_v1.jsonl" \
  --manifest-output "datasets/selector/${SELECTOR_SCOPE}/candidate_universe_manifest_v1.json" \
  --allow-decision-universe \
  --allow-incomplete-universe \
  --json
```

Jeśli masz event artifact z `datasets/events/<runtime_scope>/*.jsonl`, preferuj
`--events` jako źródło universe, a `--decisions` tylko jako kontekst.

## 15. Budowa canonical R2 source

```bash
python3 scripts/build_selector_canonical_r2_source.py \
  --root /root/Gho \
  --candidate-universe "datasets/selector/${SELECTOR_SCOPE}/candidate_universe_v1.jsonl" \
  --diag-log-glob "logs/rollout/${RUNTIME_SCOPE}/system.log*" \
  --output "datasets/selector/${SELECTOR_SCOPE}/canonical_r2_source_v1.jsonl" \
  --manifest-output "datasets/selector/${SELECTOR_SCOPE}/canonical_r2_source_manifest_v1.json" \
  --horizon-ms 60000 \
  --post-horizon-grace-ms 5000 \
  --json
```

## 16. Budowa all-decision counterfactual outcome

Dla kontraktu biznesowego 30/30/60:

```bash
python3 scripts/build_selector_all_decision_counterfactual_outcome.py \
  --root /root/Gho \
  --scope "$SELECTOR_SCOPE" \
  --runtime-scope "$RUNTIME_SCOPE" \
  --target-net-pct 30 \
  --stop-net-pct 30 \
  --horizon-ms 60000 \
  --output "datasets/selector/${SELECTOR_SCOPE}/all_decision_counterfactual_outcome_v1.jsonl" \
  --manifest-output "datasets/selector/${SELECTOR_SCOPE}/all_decision_counterfactual_outcome_manifest_v1.json" \
  --verdict-matrix-output "reports/selector/${SELECTOR_SCOPE}/all_decision_verdict_matrix_v1.csv" \
  --reason-matrix-output "reports/selector/${SELECTOR_SCOPE}/all_decision_reason_matrix_v1.csv" \
  --json
```

Ten output jest właściwszy do analiz pełnej populacji niż `shadow_lifecycle.jsonl`,
bo obejmuje decision rows oraz probe/counterfactual outcome, a nie tylko BUY
positions.

## 17. Training view / Phase3 rebuild

Jeśli posiadasz feature snapshots, accepted lifecycle i opcjonalne konteksty:

```bash
python3 scripts/build_selector_training_view.py \
  --candidate-universe "datasets/selector/${SELECTOR_SCOPE}/candidate_universe_v1.jsonl" \
  --accepted-lifecycle "datasets/selector/${SELECTOR_SCOPE}/accepted_lifecycle_v1.jsonl" \
  --feature-snapshots "datasets/selector/${SELECTOR_SCOPE}/feature_snapshots_v1.jsonl" \
  --price-paths "datasets/selector/${SELECTOR_SCOPE}/canonical_r2_source_v1.jsonl" \
  --output "datasets/selector/${SELECTOR_SCOPE}/selector_training_view_v1.jsonl" \
  --label-coverage-output "reports/selector/${SELECTOR_SCOPE}/selector_label_coverage_v1.json" \
  --leakage-audit-output "reports/selector/${SELECTOR_SCOPE}/selector_leakage_audit_v1.json" \
  --target-net-pct 30 \
  --stop-net-pct 30 \
  --horizon-ms 60000 \
  --split-denominator candidate_universe \
  --json
```

Jeśli używasz standardowego Phase3 buildera:

```bash
python3 scripts/build_selector_phase3_r2only.py \
  --scope "$SELECTOR_SCOPE" \
  --root /root/Gho \
  --gatekeeper-feature-context "datasets/selector/${SELECTOR_SCOPE}/gatekeeper_feature_context_v1.jsonl" \
  --json
```

## 18. Business label breakdown TARGET / STOP / TIMEOUT

Na gotowym pliku outcome albo training view:

```bash
python3 - <<'PY'
from pathlib import Path
import json, collections, os
scope=os.environ["SELECTOR_SCOPE"]
path=Path("/root/Gho/datasets/selector")/scope/"all_decision_counterfactual_outcome_v1.jsonl"
if not path.exists():
    path=Path("/root/Gho/datasets/selector")/scope/"selector_training_view_v1.jsonl"
c=collections.Counter()
n=0
for line in path.open(errors="ignore"):
    if not line.strip():
        continue
    row=json.loads(line)
    label=(row.get("business_label") or row.get("r2_label") or row.get("label") or row.get("outcome_label") or "UNKNOWN")
    c[str(label)]+=1
    n+=1
print("path", path)
print("rows", n)
print(c.most_common())
den=sum(c[k] for k in c if k in {"TARGET","STOP","TIMEOUT","positive","negative"})
if den:
    print("target_like_rate", (c["TARGET"]+c["positive"])/den)
PY
```

## 19. Segment Lab / analiza porównawcza

Najpierw zbuduj `zbior_A.jsonl` i `zbior_B.jsonl` zgodnie z kontraktem:

- `target_vs_not_target`: A=`TARGET`, B=`STOP+TIMEOUT`;
- `target_vs_stop`: A=`TARGET`, B=`STOP`;
- `target_vs_timeout`: A=`TARGET`, B=`TIMEOUT`;
- `stop_vs_non_stop`: A=`STOP`, B=`TARGET+TIMEOUT`;
- `timeout_vs_non_timeout`: A=`TIMEOUT`, B=`TARGET+STOP`.

Potem uruchom Sekcję 19 z artifact mode:

```bash
AB_SEGMENT_LAB=1 \
AB_SEGMENT_ENABLE_ARTIFACTS=1 \
AB_SEGMENT_MIN_SELECTED=100 \
AB_SEGMENT_TOP_N=30 \
python3 scripts/analiza_porownawcza.py \
  "reports/selector/${SELECTOR_SCOPE}/target_vs_not_target/zbior_A.jsonl" \
  "reports/selector/${SELECTOR_SCOPE}/target_vs_not_target/zbior_B.jsonl"
```

`analiza_porownawcza.py` nie ma klasycznego `--help`; przyjmuje albo dwie
ścieżki `A B`, albo jeden folder zawierający `zbior_A.jsonl` i `zbior_B.jsonl`.
Artifact mode zapisuje CSV/JSON obok wejściowego `zbior_A.jsonl`.

```bash
AB_SEGMENT_LAB=1 AB_SEGMENT_ENABLE_ARTIFACTS=1 \
python3 scripts/analiza_porownawcza.py \
  "reports/selector/${SELECTOR_SCOPE}/target_vs_not_target"
```

## 20. On-chain lifecycle validation dla BUY shadow

To dotyczy tylko BUY positions, nie pełnej populacji Gatekeepera:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config "$RUNTIME_CONFIG" \
  --output "reports/selector/${RUNTIME_SCOPE}/shadow_onchain_lifecycle_report_$(date -u +%Y%m%dT%H%M%SZ).jsonl"
```

Ten raport waliduje/correlate lifecycle pozycji z `DIAG_ACCOUNT_UPDATE_RELAY`.
Nie zastępuje all-decision outcome dla REJECT/TIMEOUT.

## 21. Najczęściej używane ścieżki runtime

BUY shadow:

```text
logs/shadow_run/<runtime_scope>/shadow_entries.jsonl
logs/shadow_run/<runtime_scope>/shadow_lifecycle.jsonl
```

Counterfactual probe:

```text
logs/shadow_run/<runtime_scope>/probe_selection.jsonl
logs/shadow_run/<runtime_scope>/probe_transport.jsonl
logs/shadow_run/<runtime_scope>/probe_shadow_entries.jsonl
logs/shadow_run/<runtime_scope>/probe_shadow_lifecycle.jsonl
logs/shadow_run/<runtime_scope>/probe_skips.jsonl
```

Decision logs:

```text
logs/rollout/<runtime_scope>/decisions/<runtime_scope>/**/gatekeeper_v2_decisions.jsonl
```

System/oracle logs:

```text
logs/rollout/<runtime_scope>/system.log.<YYYY-MM-DD>
logs/rollout/<runtime_scope>/oracle.log.<YYYY-MM-DD>
```

Events:

```text
datasets/events/<runtime_scope>/*.jsonl
```

Selector datasets:

```text
datasets/selector/<selector_scope>/candidate_universe_v1.jsonl
datasets/selector/<selector_scope>/canonical_r2_source_v1.jsonl
datasets/selector/<selector_scope>/all_decision_counterfactual_outcome_v1.jsonl
datasets/selector/<selector_scope>/selector_training_view_v1.jsonl
```

Reports:

```text
reports/selector/<runtime_or_selector_scope>/
```

## 22. Cleanup przed kolejnymi runami

Najpierw plan, dopiero potem delete:

```bash
df -h /root/Gho
du -h -d 1 /root/Gho | sort -h | tail -n 30
du -h -d 2 /root/Gho/logs /root/Gho/datasets /root/Gho/reports 2>/dev/null | sort -h | tail -n 60
```

Bezpieczne kandydaty do usunięcia zwykle są odtwarzalne build artifacts, np.:

```bash
du -sh /root/Gho/target
```

Nie usuwaj bez osobnej zgody:

- aktywnego runtime scope;
- `logs/rollout/<runtime_scope>`;
- `logs/shadow_run/<runtime_scope>`;
- `datasets/selector/<selector_scope>`;
- `datasets/events/<runtime_scope>`;
- `reports/selector/<scope>` z manifestami/checksumami.

## 23. Minimalne komendy awaryjne

Tylko status:

```bash
tmux ls || true
pgrep -af "ghost-launcher|${RUNTIME_SCOPE}|${TMUX_SESSION}" || true
df -h /root/Gho
```

Zapis listy największych plików bez usuwania:

```bash
find /root/Gho -xdev -type f -printf '%s %p\n' | sort -n | tail -n 100 > /root/Gho/largest_files_snapshot.txt
```

Ostatnie błędy:

```bash
rg -i "ResourceExhausted|stream limit|limit exceeded|panic|fatal|error|reconnect storm" \
  "logs/rollout/${RUNTIME_SCOPE}" \
  "reports/selector/${RUNTIME_SCOPE}" | tail -n 120
```

## 24. Walidacja helperów z tego runbooka

```bash
python3 -m py_compile \
  scripts/runtime_probe_coverage.py \
  scripts/runtime_fsc_coverage.py

python3 scripts/runtime_probe_coverage.py --root /root/Gho --scope "$RUNTIME_SCOPE"
python3 scripts/runtime_fsc_coverage.py --root /root/Gho --scope "$RUNTIME_SCOPE"
```
