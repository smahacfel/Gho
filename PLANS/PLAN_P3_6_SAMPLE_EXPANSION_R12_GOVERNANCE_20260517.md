# Plan P3.6 Sample Expansion R12 Governance

Data: 2026-05-17
Status: `APPROVED-FOR-EXECUTION / SAMPLE-R12-IN-PROGRESS / R12-CANDIDATE-BLOCKED / P2-BLOCKED`

## 1. Cel

Celem jest przeprowadzenie P3.6 jako kontrolowanego etapu:

```text
sample expansion
-> strict replay validation
-> outcome labels
-> sample-r12 quality report
-> combined R10+R11+sample-r12 calibration report
-> feature separation audit
-> decyzja: R12 candidate, regime analysis, P3.7 redesign albo dalsza blokada
```

Ten plan nie jest planem promocji V3.
Ten plan nie jest planem P2.
Ten plan nie jest planem live execution.
Ten plan nie jest planem tuningu progow z malej probki.

`shadow-burnin-v3-p36-sample-r12-primary-only` oznacza **sample expansion run**.
Nie oznacza calibrated R12 candidate.

P3.6 R10+R11 pokazalo, ze tooling techniczny dziala, ale obecny V3 nie ma
jeszcze kandydata nadajacego sie do R12-candidate:

- combined `known_rows=523`
- `bad_entry=126`
- `good_entry=116`
- `neutral_entry=281`
- `unknown=74`
- `protective_ratio=1.086207`
- `protective_precision=0.520661`
- `r12_gate_status=blocked`
- `p36_candidate_organic_relaxed` odblokowal `12` bad wobec `9` good

Nastepny krok to wieksza probka primary-only i analiza separacji cech, nie
promocja.

## 2. Zakres i non-goals

### In scope

- Zweryfikowac governance i invariants ADR-0130/ADR-0131.
- Monitorowac lub czysto zatrzymac aktywny `p36_sample_r12`.
- Wykonac strict full replay validation.
- Wygenerowac outcome labels dla `sample-r12`.
- Wykonac sample-r12 outcome quality report.
- Wykonac combined reports:
  - R10
  - R11
  - sample-r12
  - R10+R11
  - R11+sample-r12
  - R10+R11+sample-r12
- Dodac planowany wrapper `scripts/v3_p36_feature_separation_audit.py`.
- Uzyc legacy `logs/decisions.json/analiza_porownawcza.py` jako offline appendix,
  nie jako generator progow runtime.
- Podjac decyzje wedlug jawnych gates.

### Out of scope

- No P2.
- No live trading.
- No live sender changes.
- No active V2/V2.5 policy changes.
- No IWIM changes.
- No calibrated R12 candidate run przed offline gate.
- No FSC/full-chain funding claim.
- No authoritative FSC dependency.
- No negative interpretation of missing/degraded FSC.
- No threshold tuning from 23-row organic-relaxed candidate.
- No implementation of generated thresholds from `analiza_porownawcza.py`.
- No retrospective mutation of R10/R11 evidence/configs.

## 3. Stan wejsciowy

Wykonawca ma zaczac od zapisania stanu:

```bash
pwd
git rev-parse --show-toplevel
git rev-parse HEAD
git status --short
```

Oczekiwany HEAD w chwili tworzenia planu:

```text
2c6ebcaead5c4b8764a40fe0f73cb2a13c5281da
```

Commit:

```text
Add P3.6 sample expansion governance
```

Znane lokalne nie sledzone pliki poza zakresem tego planu:

```text
configs/rollout/shadow-burnin-v3-p32-replay-r4.toml
configs/rollout/shadow-burnin-v3-p32-replay-r5.toml
configs/rollout/shadow-burnin-v3-p32-replay-r6.toml
```

Nie ruszac ich, chyba ze osobne zadanie jawnie wlaczy je w zakres.

## 4. Pliki i artefakty

### Dokumenty zrodlowe do inspekcji

```text
docs/ADR/ADR-0130-v3-fsc-scope-decision-single-stream.md
docs/ADR/ADR-0131-v3-p36-sample-expansion-runtime-governance.md
PLANS/PLAN_P3_6_V3_SHADOW_ONLY_CALIBRATION_20260517.md
PLANS/AUDYT/RAPORT_P3_6_V3_SHADOW_CALIBRATION_R10_R11_20260517.md
PLANS/AUDYT/RAPORT_OPERACYJNY_P3_5_V3_OUTCOME_QUALITY_R10_20260516.md
PLANS/AUDYT/RAPORT_OPERACYJNY_P3_5_V3_OUTCOME_QUALITY_R11_20260516.md
```

### Configi

```text
configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml
configs/rollout/ghost_brain_v3_p36_primary_only.toml
configs/rollout/shadow-burnin-v3-p36-calibrated-r12-primary-only.toml
configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml
configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml
ghost-brain/ghost_brain_config.toml
```

### Skrypty i binaria

```text
ghost-launcher/src/bin/v3_replay.rs
scripts/v3_shadow_report.py
scripts/v3_full_replay_report.py
scripts/v3_outcome_quality_report.py
scripts/v3_p36_calibration_report.py
scripts/gatekeeper_outcome_labeler.py
logs/decisions.json/rollout/shadow-burnin/decisions/fetch_pool_price_at_30s.py
logs/decisions.json/analiza_porownawcza.py
```

### Planowany nowy wrapper

```text
scripts/v3_p36_feature_separation_audit.py
```

Wrapper ma byc repo-owned source file. Legacy analyzer w `logs/` ma pozostac
niezmieniony, chyba ze osobna decyzja przeniesie go do `scripts/legacy/`.

### Namespace sample-r12

```text
logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/
logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/
logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/reports/
logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/reports/health/
logs/shadow_run/shadow-burnin-v3-p36-sample-r12-primary-only/
datasets/events/shadow-burnin-v3-p36-sample-r12-primary-only/
data/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/
```

## 5. Twarde kontrakty

### ADR-0130 / FSC

Pod single-stream provider constraint:

- `funding_lane_mode="disabled"`
- brak drugiego funding-chain streamu
- brak authoritative FSC dependency
- missing/degraded FSC nie jest negatywnym sygnalem
- FSC nie jest promotion prerequisite
- FSC nie jest required evidence source
- FSC fields moga byc raportowane tylko jako `excluded_by_adr_0130`

### ADR-0131 / sample-r12

`sample-r12` jest evidence gathering, nie promotion:

- `entry_mode="shadow_only"`
- `execution_mode="shadow"`
- `gatekeeper_v3.enabled=false`
- `gatekeeper_v3.shadow_emit_enabled=true`
- `gatekeeper_v3.replay_payload_enabled=true`
- `gatekeeper_v3.promotion.enabled=false`
- `gatekeeper_v3.evidence_requirements.fsc=false`
- no active V2/V2.5 policy change
- no IWIM change
- no live sender change
- no P2 promotion

### Replay i labels

- `v3_shadow_report.py` jest health/freshness/coverage/distribution check.
- `v3_full_replay_report.py --strict` jest certification gate.
- Labels wolno generowac dopiero po clean stop i strict full replay OK.
- Neutral nigdy nie jest mieszany z good/bad.
- `PENDING` jest liczony jako effective block i raportowany osobno od terminal
  `REJECT`.
- `status=ok` raportu nie oznacza promotion gate pass.
- O decyzji decyduje `r12_gate_status` i konkretne blockers.

## 6. Etap A - repo i governance sanity

### Cel

Upewnic sie, ze sample expansion nie narusza zadnego kontraktu i ze wykonawca
pracuje na oczekiwanym HEAD.

### Komendy

```bash
git rev-parse HEAD
git status --short
test -f docs/ADR/ADR-0130-v3-fsc-scope-decision-single-stream.md
test -f docs/ADR/ADR-0131-v3-p36-sample-expansion-runtime-governance.md
test -f PLANS/PLAN_P3_6_V3_SHADOW_ONLY_CALIBRATION_20260517.md
test -f PLANS/AUDYT/RAPORT_P3_6_V3_SHADOW_CALIBRATION_R10_R11_20260517.md
test -f configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml
test -f configs/rollout/ghost_brain_v3_p36_primary_only.toml
```

Zweryfikowac invariants w configach:

```bash
rg -n 'funding_lane_mode|entry_mode|execution_mode|gatekeeper_v3|shadow_emit_enabled|replay_payload_enabled|promotion|evidence_requirements|fsc' \
  configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  configs/rollout/ghost_brain_v3_p36_primary_only.toml
```

Sprawdzic, czy historyczne configi i active brain config nie maja nieoczekiwanych
lokalnych zmian:

```bash
git diff -- \
  ghost-brain/ghost_brain_config.toml \
  configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml \
  configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml
```

### Artefakt governance sanity

Zapisac JSON:

```text
logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/reports/governance_sanity_<timestamp>.json
```

Minimalna struktura:

```json
{
  "timestamp_utc": "...",
  "git_head": "2c6ebcaead5c4b8764a40fe0f73cb2a13c5281da",
  "worktree_status": "...",
  "sample_config": "configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml",
  "ghost_brain_config": "configs/rollout/ghost_brain_v3_p36_primary_only.toml",
  "adr_0130_present": true,
  "adr_0131_present": true,
  "invariants": {
    "funding_lane_mode_disabled": true,
    "entry_mode_shadow_only": true,
    "execution_mode_shadow": true,
    "gatekeeper_v3_enabled_false": true,
    "shadow_emit_enabled_true": true,
    "replay_payload_enabled_true": true,
    "promotion_enabled_false": true,
    "fsc_required_false": true
  },
  "historical_configs_unchanged": true,
  "active_runtime_changes_detected": false,
  "notes": []
}
```

### Acceptance

- HEAD zapisany.
- Wszystkie wymagane dokumenty istnieja.
- Wszystkie invariants ADR-0131 sa true.
- Historyczne R10/R11 configi nie sa mutowane.
- Brak zmian active V2/V2.5, IWIM, live sender, execution.

### Failure handling

Jesli ktorykolwiek invariant failuje, zatrzymac prace i opisac drift.
Nie przechodzic do monitoringu/stop/replay.

## 7. Etap B - kontrola aktywnego sample-expansion runu

### Cel

Bezpiecznie kontynuowac istniejacy run lub przygotowac go do clean stop bez
restartowania namespace.

### Stan oczekiwany z ADR-0131

```text
tmux session: p36_sample_r12
launcher pid: 1450578
namespace: shadow-burnin-v3-p36-sample-r12-primary-only
```

PID moze sie zmienic w przyszlosci, ale namespace i tmux session powinny byc
traktowane jako SSOT operacyjny dla tego runu.

### Komendy kontroli procesu

```bash
tmux ls
pgrep -af 'ghost-launcher|target/release/ghost-launcher|cargo run'
```

Monitor:

```bash
tmux attach -t p36_sample_r12
tmux select-window -t p36_sample_r12:monitor
```

### Health report

Co 30-60 minut:

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  --json
```

Zapisac kazdy snapshot pod:

```text
logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/reports/health/health_<timestamp>.json
```

Kazdy snapshot musi zawierac lub pozwalac odtworzyc:

- `timestamp_utc`
- `git_head`
- `config_path`
- `pid`
- namespace disk usage
- decision log path
- decision log mtime
- `raw_rows`
- `deduped_rows`
- `v3_rows`
- `replay_status`
- `full_snapshot_payload_rows`
- `hash_only_rows`
- `stale_against_config`
- `policy_hash_unique_count`
- `snapshot_hash_unique_count`
- top V3 reason distribution
- active->V3 matrix

Disk usage:

```bash
du -sh logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only \
  logs/shadow_run/shadow-burnin-v3-p36-sample-r12-primary-only \
  datasets/events/shadow-burnin-v3-p36-sample-r12-primary-only \
  data/rollout/shadow-burnin-v3-p36-sample-r12-primary-only 2>/dev/null || true
```

Decision log mtime:

```bash
find logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions \
  -name gatekeeper_v2_decisions.jsonl -printf '%T@ %p\n' | sort -n | tail -5
```

### Stop gate

Minimalny stop gate:

```text
v3_rows >= 1250
AND replay_status=full
AND stale_against_config=false
AND full_snapshot_payload_rows == v3_rows
AND hash_only_rows == 0
```

Preferowany stop gate:

```text
v3_rows between 1500 and 3000
AND stable full replay health
AND no stream/runtime degradation
AND no policy hash drift
```

Uzasadnienie: jezeli label coverage bedzie okolo `0.80`, to `1000` rows daje
tylko okolo `800` known labels. Aby celowac w `known_outcome_rows >= 1000`,
minimum powinno wynosic okolo `1250` rows, a praktyczny cel to `1500-3000`.

### Slow growth handling

Jesli `v3_rows` nie rosna lub rosna bardzo wolno:

1. Nie restartowac od razu.
2. Sprawdzic `tmux_launcher.log`.
3. Sprawdzic `system.log` i `oracle.log`.
4. Sprawdzic stream health.
5. Sprawdzic disk usage.
6. Sprawdzic czy decision log mtime sie zmienia.
7. Dopiero potem podjac decyzje operatorska.

### Acceptance

- Istniejacy run jest monitorowany w tym samym namespace.
- Health snapshots zapisywane sa jako artefakty.
- Stop gate jest jawny.
- Nie wygenerowano labels podczas aktywnego runu.

## 8. Etap C - clean stop

### Cel

Zamknac run bez zostawienia aktywnych procesow i bez zanieczyszczenia artefaktow.

### Stop

```bash
tmux send-keys -t p36_sample_r12:0 C-c
```

### Kontrola po stopie

```bash
pgrep -af 'ghost-launcher|target/release/ghost-launcher|cargo run'
pgrep -af 'fetch_pool_price_at_30s.py|gatekeeper_outcome_labeler.py|v3_outcome_quality_report.py'
tmux ls
```

### Acceptance

- Brak aktywnego `ghost-launcher`.
- Brak aktywnego `cargo run` powiazanego z runem.
- Brak aktywnego fetchera/labelera/quality reportera, ktory moglby zapisac do
  starego lub obcego checkpoint/output.
- Artefakty pozostaja w namespace:

```text
shadow-burnin-v3-p36-sample-r12-primary-only
```

### Failure handling

Jesli proces zostaje aktywny, nie przechodzic do strict replay.
Najpierw ustalic, czy to:

- ten sam sample run,
- inny niezalezny proces,
- stary fetcher/labeler,
- albo operator-intentional process.

## 9. Etap D - strict replay validation

### Cel

Potwierdzic, ze sample run jest full replay, a nie hash-only.

### Semantyka narzedzi

`v3_shadow_report.py`:

- health
- coverage
- freshness
- row distribution
- reason distribution
- hash coverage

`v3_full_replay_report.py --strict`:

- certification gate
- row-level full replay validation
- warunek dopuszczenia labeling pipeline

### Komendy

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  --json

python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  --strict \
  --json
```

### Acceptance

Wymagane:

```text
status=ok
replay_status=full_replay_ok
bad_rows=0
hash_only_rows=0
full_snapshot_payload_rows == v3_rows
stale_against_config=false
policy_hash_unique_count=1
```

Preferowane:

```text
snapshot_hash_unique_count ~= v3_rows
duplicate_rows_removed=0 albo wyjasnione
pre_dedupe_conflicts.conflict_groups=0
```

### Failure handling

Jesli strict replay failuje:

- nie generowac labels,
- nie uruchamiac outcome quality report,
- nie wlaczac sample-r12 do combined,
- najpierw naprawic replay/payload/config mismatch.

## 10. Etap E - outcome labeling sample-r12

### Cel

Dolaczyc labels w tym samym jezyku co R10/R11, bez mieszania klas i bez
checkpoint drift.

### Znalezienie decision log

Najpierw uzyc `v3_shadow_report.py --json` i pola `inputs.decisions_log`.
Nie zgadywac sciezki, jesli report zwraca konkretna.

Fallback:

```bash
find logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions \
  -name gatekeeper_v2_decisions.jsonl -printf '%T@ %p\n' | sort -n | tail -1
```

### Threshold hits

```bash
python3 logs/decisions.json/rollout/shadow-burnin/decisions/fetch_pool_price_at_30s.py \
  <decision_log> \
  --output logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/p3_6_pool_threshold_hits.jsonl \
  --checkpoint logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/p3_6_pool_threshold_hits.checkpoint.jsonl \
  --workers 4 \
  --rps 20
```

Jesli RPC/timeout przerywa, wznowic z checkpointem. Nie resetowac checkpointu na
sile.

### Label generation

```bash
python3 scripts/gatekeeper_outcome_labeler.py \
  --decisions <decision_log> \
  --threshold-hits logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/p3_6_pool_threshold_hits.jsonl \
  --output logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/p3_6_gatekeeper_plus40_labels.jsonl
```

### Labeling metadata

Raport labelingowy musi zapisac:

- `checkpoint_resumed=true|false`
- `workers=4`
- `rps=20`
- final output path
- threshold rows
- label rows
- unresolved count
- unresolved reasons
- `entry_price_unavailable` count
- match quality: `tight`, `usable`, `degraded`, albo `failed`

### Coverage gate

Target:

```text
coverage >= 0.85
```

Minimum dla candidate-selection aggregate:

```text
coverage >= 0.80
```

Jesli coverage `<0.80`:

- nadal wygenerowac diagnostic report,
- nie wyrzucac runu operacyjnie,
- nie uzywac sample-r12 w promotion-quality aggregate,
- nie uzywac sample-r12 do candidate selection,
- opisac unresolved reasons i match quality.

### Acceptance

- Labels sa w namespace sample-r12.
- Coverage jawnie raportowane.
- Neutral nie jest mieszany z good/bad.
- `entry_price_unavailable` jest osobno.
- Labels nie sa generowane, jesli strict replay failuje.

## 11. Etap F - sample-r12 outcome quality report

### Cel

Zrozumiec sample-r12 samodzielnie przed scaleniem z R10/R11.

### Komenda

```bash
python3 scripts/v3_outcome_quality_report.py \
  --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  --outcome-labels logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/p3_6_gatekeeper_plus40_labels.jsonl \
  --json
```

### Wymagane metryki

- `v3_rows`
- `known_outcome_rows`
- coverage
- `bad_entry`
- `good_entry`
- `neutral_entry`
- `unknown`
- `avoided_bad`
- `blocked_good`
- `protective_ratio`
- `protective_precision`
- reason breakdown
- active->V3 breakdown
- selected good entries
- selected bad entries
- PENDING as effective block
- PENDING osobno od terminal REJECT

### Acceptance

Acceptance nie oznacza dobrego ratio. Acceptance oznacza czysty evidence run:

- strict replay OK,
- labels generated,
- coverage reported,
- unresolved reasons listed,
- sample outcome report generated.

## 12. Etap G - combined R10/R11/sample-r12 calibration reports

### Cel

Rozszerzyc P3.6 z R10+R11 do sample-r12 i sprawdzic stabilnosc wynikow po runach.

### Existing R10/R11 labels

Uzyc istniejacych label files:

```text
logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
```

Nie regenerowac R10/R11 labels bez nowego powodu.

### Combined all

```bash
python3 scripts/v3_p36_calibration_report.py \
  --run r10:configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --run r11:configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --run sample_r12:configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml:logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/p3_6_gatekeeper_plus40_labels.jsonl \
  --json
```

### Combined recent

```bash
python3 scripts/v3_p36_calibration_report.py \
  --run r11:configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --run sample_r12:configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml:logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/p3_6_gatekeeper_plus40_labels.jsonl \
  --json
```

### Wymagane agregacje

Raport ma pokazac osobno:

- R10 standalone
- R11 standalone
- sample-r12 standalone
- R10+R11
- R11+sample-r12
- R10+R11+sample-r12

### Wymagane metryki

- `known_rows`
- `bad_entry`
- `good_entry`
- `neutral_entry`
- `unknown`
- `avoided_bad`
- `blocked_good`
- `protective_ratio`
- `protective_precision`
- reason-level outcome split
- subtrigger outcome split
- `PENDING` effective block split
- organic failure split
- candidate BUY analysis
- per-run stability
- combined stability
- `r12_gate_status`
- `blocked_gates`

### Acceptance

- Combined reports generated.
- `neutral_entry` nie jest mieszany z `good_entry`/`bad_entry`.
- `PENDING` jest effective block i osobna kategoria.
- Decyzja promotion nie bazuje na `status=ok`, tylko na `r12_gate_status`.
- `combined_recent` jest jawnie raportowany, aby R10 nie maskowal slabszego R11
  albo nowszego sample-r12.

## 13. Etap H - feature separation audit z analiza_porownawcza.py

### Cel

Sprawdzic, czy w decision-time-safe feature set istnieje realna separacja
miedzy dobrymi i zlymi entries po tym, jak proste ablation P3.6 nie znalazlo
bezpiecznego kandydata.

### Rola legacy analyzer

Legacy script:

```text
logs/decisions.json/analiza_porownawcza.py
```

jest uzyteczny jako offline A/B exploratory analyzer:

- A/B filtering
- dedup po `ab_record_id`
- rozklady
- korelacje
- effect size
- Mann-Whitney
- AUC
- Youden J
- KS
- overlap
- bootstrap CI
- L1 logistic regression
- scoring-rule exploration
- optional DTW/TDA/MI/Hill
- Sybil Interference appendix

Ale:

- nie jest source of truth,
- nie jest generator progow do Gatekeepera,
- nie moze implementowac runtime rules,
- nie moze uzywac FSC jako aktywnego feature rankingu,
- nie dowodzi przyczynowosci.

### Nowy wrapper

Dodac:

```text
scripts/v3_p36_feature_separation_audit.py
```

Wrapper ma:

1. Przyjmowac input:

```text
--run name:config:labels
--comparison <comparison_name>
--variant <variant_name>
--output-dir <path>
--json
--markdown
```

2. Wczytac decision JSONL rozpoznany przez config/report.
3. Wczytac labels.
4. Joinowac po `ab_record_id`.
5. Deduplikowac po `ab_record_id`.
6. Oddzielac `neutral_entry` i `unknown` od A/B.
7. Flattenowac tylko whitelistowane decision-time-safe fields.
8. Tworzyc A/B JSONL:

```text
A_good_entry.jsonl
B_bad_entry.jsonl
A_good_unblocked.jsonl
B_bad_unblocked.jsonl
```

9. Uruchamiac legacy analyzer jako subprocess.
10. Zapisac:

```text
comparison_summary.json
comparison_summary.md
analiza_stdout.txt
analiza_<timestamp>.html
```

11. Oznaczac male probki:

```text
if n_A < 50 or n_B < 50:
  status=hypothesis_only
  threshold_recommendation_allowed=false
```

### Mandatory comparisons

1. `good_entry` vs `bad_entry` na R10+R11+sample-r12.
2. `good_entry` vs `bad_entry` na R11+sample-r12.
3. `p36_candidate_organic_relaxed`: `good_unblocked` vs `bad_unblocked`.
4. `REJECT_V3_MANIPULATION_CONTRADICTION`: good vs bad.
5. `PENDING_V3_WAIT_EVIDENCE`: good vs bad.
6. Organic failure groups: good vs bad.
7. Per-run comparison:
   - R11 standalone
   - sample-r12 standalone
   - combined all

### Mandatory output fields

Kazde porownanie musi raportowac:

- `comparison_name`
- `runs_included`
- `n_A`
- `n_B`
- `neutral_excluded`
- `unknown_excluded`
- `sample_size_warning`
- `hypothesis_only`
- top feature deltas
- AUC ranking
- overlap
- bootstrap CI status
- stability by run
- `threshold_recommendation_allowed=false`
- FSC excluded list

### Feature whitelist

#### tx_intel

```text
tx_count
buy_count
unique_signers
buy_ratio
hhi
top3_volume_pct
same_ms_tx_ratio
bundle_suspicion_ratio
dev_volume_ratio
max_tx_per_signer
total_volume_sol
avg_tx_sol
volume_cv
```

#### organic_broadening

```text
buy_ratio_min
buy_ratio_mean
buy_ratio_max
tx_count_growth_ratio
unique_signer_growth_ratio
new_signer_ratio_t2
hhi_delta_t2_t0
max_segment_hhi
t1_vs_t0_unique_signer_delta
t2_vs_t1_unique_signer_delta
```

#### manipulation_contradictions

```text
same_ms_tx_ratio
bundle_suspicion_ratio
top3_volume_pct
hhi
dev_volume_ratio
contradiction_score
timing_bundle_concentration
high_buy_pressure_with_high_top3
fixed_size_or_ramping_pattern
early_top3_concentration
```

#### tas/checkpoint

```text
overall_tas_score
momentum_score
hhi_score
volume_score
interval_score
buy_ratio_score
```

#### alpha

```text
fixed_size_buy_ratio
flipper_presence_ratio
compute_unit_cluster_dominance
static_fee_profile_ratio
jito_tip_intensity
early_top3_buy_volume_pct_3s
```

#### non-FSC sybil

```text
fee_topology_diversity_index
signer_cross_pool_velocity
spend_fraction_divergence
demand_elasticity_score
```

#### excluded_by_adr_0130

```text
funding_source_concentration
funding_source_diagnostics
```

FSC fields moga byc obecne tylko w sekcji excluded/diagnostic-only.
Nie wolno ich uzywac jako feature ranking dla aktualnej kalibracji P3.6.

### Interpretacja wynikow

Feature moze zostac kandydatem do dalszej ablation tylko jezeli:

- `n_good >= 50`
- `n_bad >= 50`
- kierunek efektu jest zgodny w R11 i sample-r12
- efekt utrzymuje sie w combined all
- bootstrap CI nie jest bardzo szeroki
- overlap nie jest bardzo wysoki
- feature istnieje w `MaterializedFeatureSet`
- feature jest decision-time-safe
- feature nie jest post-hoc
- feature nie jest FSC-dependent pod ADR-0130

Male probki, np. 23 BUY candidates z organic-relaxed:

```text
status=hypothesis_only / falsification_only
no threshold tuning
no promotion
```

### Twardy zakaz progow z analyzer

Sekcje legacy analyzer:

- `Optymalne Progi Separujace`
- `Youden J`
- `Gotowa Regula Decyzyjna`
- `L1 logistic regression`
- `Scoring Rule`

sa tylko appendixem hipotez.

Zaden prog ani regula nie moze zostac zaimplementowana bez:

1. jawnej hipotezy,
2. counterfactual full replay ablation,
3. outcome-quality report,
4. combined R10/R11/sample-r12 gate,
5. osobnego planu implementacyjnego,
6. braku regresji shadow/live boundary.

## 14. Etap I - decision gates po combined i feature audit

### Sciezka 1 - nadal brak kandydata

Warunki:

- zaden wariant nie poprawia ratio,
- `bad_unblocked > 0.5 * good_unblocked`,
- candidate BUY rows sa male,
- feature audit nie pokazuje stabilnej separacji,
- poprawa nie utrzymuje sie w R11 i sample-r12 osobno.

Decyzja:

- `R12-candidate=blocked`
- `P2=blocked`
- nie robic kolejnego blind sample run bez nowej hipotezy
- przejsc do P3.7 feature redesign

### Sciezka 2 - offline candidate allowed

Minimalne warunki:

```text
strict replay OK for every run
label coverage >= 0.85
candidate known rows >= 100
protective_ratio >= 1.30
blocked_good decreases >= 10% OR >= 25 rows
bad_unblocked <= 0.5 * good_unblocked
unknown_unblocked <= 0.25 * good_unblocked
neutral_unblocked reported separately
no active policy change
no P2 promotion
```

Stability condition:

Poprawa musi trzymac sie w co najmniej:

- R11 standalone
- sample-r12 standalone
- R10+R11+sample-r12 combined

Nie wystarcza, ze combined wyglada dobrze, jezeli caly efekt pochodzi z R10
albo jednego krotkiego regime.

Decyzja:

- zaprojektowac osobny calibrated candidate profile,
- nie uruchamiac live,
- uruchomic dopiero R12-candidate shadow-only,
- wymagac osobnego ADR/plan przed P2.

### Sciezka 3 - regime split

Warunki:

- sample-r12 ma istotnie inny reason/outcome distribution niz R10/R11,
- combined all maskuje rozjazd,
- candidate dziala tylko w jednym regime.

Decyzja:

- nie laczyc bez segmentacji,
- wykonac multi-regime analysis,
- podzielic po timestamp/regime/market activity,
- nie promowac candidate przed stabilnoscia miedzy segmentami.

### Sciezka 4 - tool falsification / no separability

Warunki:

- feature separation audit nie pokazuje stabilnej separacji good vs bad,
- organic/manipulation/PENDING feature families wygladaja symetrycznie,
- top features maja wysoki overlap lub niestabilny kierunek miedzy runami.

Decyzja:

- zamknac obecna family of gates jako nieperspektywiczna,
- zatrzymac threshold-level calibration,
- otworzyc P3.7 feature redesign,
- szukac nowego evidence zamiast luzowac istniejace progi.

## 15. P3.7 feature redesign - warunkowy nastepny etap

Jesli sample-r12 potwierdzi brak bezpiecznego kandydata:

```text
If sample-r12 confirms no safe candidate:
  stop threshold-level calibration
  open P3.7 feature redesign
  focus on new decision evidence, not loosening existing gates
```

Mozliwe kierunki P3.7:

- lepsza definicja early good opportunity,
- nowe trajectory features,
- lifecycle-aware labels,
- MFE/MAE zamiast tylko +40 threshold,
- execution feasibility joins,
- rozdzielenie neutral/no-target od real failed opportunity,
- bardziej przyczynowa dekompozycja PDD/organic/manipulation,
- feature families, ktore istnieja w `MaterializedFeatureSet` i sa
  decision-time-safe.

P3.7 nie jest automatycznie P2.
P3.7 nie moze reaktywowac legacy HyperPrediction/Chaos jako active path.

## 16. Minimalne warunki rozmowy o P2

P2 pozostaje zablokowane.

Rozmowe o P2 wolno otworzyc dopiero przy:

- multi-run full replay OK,
- outcome label coverage `>=0.85`,
- candidate shadow profile, nie baseline,
- candidate protective ratio `>=1.30`, preferowane wyzej,
- materialnie nizsze `blocked_good`,
- kontrolowane `bad_unblocked`,
- BUY candidate sample wystarczajaco duzy do precision estimate,
- shadow execution/lifecycle quality included,
- no active policy regressions,
- no live execution regressions,
- osobny ADR promotion.

Obecny P3.6 tego nie spelnia.

## 17. Komendy kontrolne

### Compile/syntax

```bash
python3 -m py_compile \
  scripts/v3_shadow_report.py \
  scripts/v3_full_replay_report.py \
  scripts/v3_outcome_quality_report.py \
  scripts/v3_p36_calibration_report.py \
  scripts/gatekeeper_outcome_labeler.py
```

Po dodaniu wrappera:

```bash
python3 -m py_compile scripts/v3_p36_feature_separation_audit.py
```

Nie wymagac py_compile dla `logs/decisions.json/analiza_porownawcza.py` jako
warunku planu, chyba ze wrapper ma go wywolywac w danym runie.

### Rust targeted checks

```bash
cargo test -p ghost-launcher --bin v3_replay
cargo test -p ghost-brain --test ghost_brain_config_load_test
```

Nie uzywac `cargo test --workspace` jako domyslnego checka dla tego etapu, chyba
ze osobny zakres tego wymaga.

### Reports

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  --json

python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  --strict \
  --json

python3 scripts/v3_outcome_quality_report.py \
  --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  --outcome-labels logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/p3_6_gatekeeper_plus40_labels.jsonl \
  --json

python3 scripts/v3_p36_calibration_report.py \
  --run r10:configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --run r11:configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --run sample_r12:configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml:logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/decisions/p3_6_gatekeeper_plus40_labels.jsonl \
  --json

git diff --check
```

## 18. Acceptance criteria

### Operational acceptance

- Governance sanity artifact exists.
- Sample run health snapshots exist.
- Clean stop confirmed.
- No active launcher/fetcher/labeler before labeling.
- Strict replay passed.
- Outcome labels generated or diagnostic failure explained.
- Sample outcome report generated.
- Combined all and combined recent generated.

### Evidence acceptance

- `replay_status=full_replay_ok`
- `bad_rows=0`
- `hash_only_rows=0`
- `full_snapshot_payload_rows == v3_rows`
- `stale_against_config=false`
- `policy_hash_unique_count=1`
- label coverage reported
- neutral separated
- PENDING effective block reported separately
- candidate variants evaluated with outcome deltas
- `r12_gate_status` reported directly

### Candidate acceptance

R12 candidate remains blocked unless all are true:

- strict replay OK for every included run,
- label coverage `>=0.85`,
- known candidate rows `>=100`,
- candidate protective ratio `>=1.30`,
- blocked good decreases materially,
- bad unblocked controlled,
- unknown unblocked controlled,
- stability holds in R11, sample-r12 and combined all,
- no active behavior change,
- no P2 promotion.

### P3.7 trigger

Open P3.7 feature redesign if:

- sample-r12 confirms no safe candidate,
- feature separation audit shows no stable good/bad separation,
- organic/manipulation/PENDING remain symmetric blockers,
- threshold-level calibration would become blind tuning.

## 19. Ryzyka i mitigacje

### Ryzyko: sample-r12 mylone z calibrated-r12

Mitigacja:

- w kazdym raporcie pisac `sample expansion`, nie `candidate run`,
- calibrated candidate pozostaje blocked przed offline gate.

### Ryzyko: tuning z 23-row sample

Mitigacja:

- `n_A < 50` lub `n_B < 50` oznacza `hypothesis_only`,
- no threshold recommendation,
- no promotion.

### Ryzyko: FSC wraca jako aktywny sygnal

Mitigacja:

- FSC tylko `excluded_by_adr_0130`,
- no FSC ranking,
- no FSC hard gate,
- no negative missing/degraded FSC.

### Ryzyko: v3_shadow_report mylony ze strict replay

Mitigacja:

- `v3_shadow_report.py` = health,
- `v3_full_replay_report.py --strict` = certification.

### Ryzyko: low coverage

Mitigacja:

- coverage `<0.80` nadal diagnostic report,
- exclude from candidate-selection aggregate,
- unresolved reasons listed.

### Ryzyko: R10 maskuje nowsze wyniki

Mitigacja:

- wymagac `combined_recent=R11+sample-r12`,
- wymagac stability by R11, sample-r12 and combined all.

### Ryzyko: analyzer generuje atrakcyjne progi

Mitigacja:

- analyzer output is appendix only,
- no generated threshold without full replay ablation.

## 20. Delegation trace

```yaml
delegation_trace:
  task_classification: "P3.6 sample expansion governance and feature separation audit execution plan"
  routing_performed: true
  primary_specialist: "Decision Logging Replay Analyst"
  supporting_specialists_considered:
    - "Config Rollout Safety Reviewer"
    - "Gatekeeper Policy Auditor"
    - "Ghost Runtime Coordinator"
  specialist_docs_loaded:
    - "docs/agents/decision-logging-replay-analyst.md"
    - "docs/agents/config-rollout-safety-reviewer.md"
  specialist_docs_not_loaded:
    - name: "gatekeeper-policy-auditor"
      reason: "plan does not change active policy; it defines offline replay, labels, candidate gates and audit workflow"
    - name: "solana-execution-path-engineer"
      reason: "scope explicitly excludes live sender and execution changes"
    - name: "seer-ingest-event-integrity-specialist"
      reason: "stream health is monitored operationally, but ingest semantics are not modified"
  skills_used:
    - "ghost-execution"
    - "trading-systems"
    - "statistical-research-engine"
    - "large-data-analytics"
  fast_path_used: false
  runtime_area_touched:
    - "shadow-only rollout governance"
    - "DecisionLogger JSONL evidence"
    - "V3 full replay"
    - "outcome labels"
    - "offline feature separation audit"
  contracts_checked:
    - "MaterializedFeatureSet remains canonical decision snapshot"
    - "shadow/live separation"
    - "DecisionLogger/replay evidence"
    - "config rollout safety"
    - "ADR-0130 FSC de-scope"
    - "ADR-0131 sample expansion only"
    - "no P2/no promotion"
    - "neutral outcome separation"
    - "PENDING effective block separation"
  active_or_legacy_path: "shadow-only V3 sidecar and offline replay; legacy analyzer used only as offline appendix"
  risk_level: "medium"
  unresolved_routing_uncertainty: []
```

## 21. Finalna zasada

P3.6 sample expansion ma odpowiedziec na jedno pytanie:

```text
Czy na wiekszej probce da sie znalezc stabilna, decision-time-safe separacje
miedzy dobrymi i zlymi entries, ktora nie jest tylko blokowaniem wszystkiego?
```

Jesli tak, nastepny krok to osobny shadow-only R12 candidate.
Jesli nie, nastepny krok to P3.7 feature redesign, nie kolejne luzowanie progow.
