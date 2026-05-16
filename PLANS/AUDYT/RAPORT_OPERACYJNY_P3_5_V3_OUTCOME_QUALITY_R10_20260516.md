# Raport operacyjny P3.5 V3 Outcome Quality r10 primary-only

Data: 2026-05-16
Namespace: `shadow-burnin-v3-p32-replay-r10-primary-only`
Config: `configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml`
Status: `OUTCOME-QUALITY-READY / P2-NO-GO`

## Cel

R10 zostal uruchomiony jako rozszerzenie probki P3.5 po r9.

Celem nie byla promocja V3, tuning progow ani wlaczenie FSC. Celem bylo powiekszenie primary-only
full replay evidence i policzenie sponsor-readable outcome quality:

- ile zlych wejsc V3 zablokowal,
- ile dobrych wejsc V3 zablokowal,
- ile decyzji bylo neutralnych,
- ile pozostalo niekonkluzywnych.

## Kontrakt runtime

- `entry_mode=shadow_only`
- `execution_mode=shadow`
- `seer.stream_mode=single_global`
- `seer.funding_lane_mode=disabled`
- `gatekeeper_v3.enabled=false`
- `gatekeeper_v3.shadow_emit_enabled=true`
- `gatekeeper_v3.replay_payload_enabled=true` przez izolowany brain config
- `promotion.enabled=false`
- brak P2 promotion
- brak zmian active V2/V2.5, IWIM, execution/live sender

## Przebieg

Preflight poczatkowo zablokowal run z powodu starego lokalnego baseline stamp:

```text
baseline.accepted_revision: expected 9eaf2a7... but stamp contains 6bd739...
```

Wykonano wymagany precheck:

```bash
cargo test --workspace --no-run
```

Po zielonym prechecku odswiezono lokalny `.ghost/baseline_accepted_revision` i ponowiono preflight.
Preflight przeszedl.

R10 uruchomiono komenda:

```bash
timeout 30m env RUST_LOG=info \
  cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml
```

Proces zakonczyl sie naturalnym timeoutem:

- exit code: `124`
- brak aktywnego `ghost-launcher` / `cargo run` po zakonczeniu
- brak drugiego Yellowstone streamu

## Full replay

Komenda:

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml \
  --strict \
  --json
```

Wynik:

- `status=ok`
- `replay_status=full_replay_ok`
- `total_rows=150`
- `v3_rows=150`
- `bad_rows=0`
- `status_counts.full_replay_ok=150`

Interpretacja:

R10 jest poprawnym full replay proof dla 150 V3 rows. Ten etap nie jest hash-only.

## Outcome labels

Decision log:

```text
logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/shadow-burnin-v3-p32-replay-r10-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl
```

Threshold hits wygenerowano przez:

```bash
python3 logs/decisions.json/rollout/shadow-burnin/decisions/fetch_pool_price_at_30s.py \
  logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/shadow-burnin-v3-p32-replay-r10-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl \
  --output logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_pool_threshold_hits.jsonl \
  --checkpoint logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_pool_threshold_hits.checkpoint.jsonl \
  --workers 8 \
  --rps 50 \
  --no-resume
```

Pierwszy przebieg zostal przerwany technicznie przy checkpoint `119/150`. Wznowiono go z tym samym
checkpointem, ale z nizszym obciazeniem RPC:

```bash
python3 logs/decisions.json/rollout/shadow-burnin/decisions/fetch_pool_price_at_30s.py \
  logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/shadow-burnin-v3-p32-replay-r10-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl \
  --output logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_pool_threshold_hits.jsonl \
  --checkpoint logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_pool_threshold_hits.checkpoint.jsonl \
  --workers 4 \
  --rps 20
```

Finalny output:

```text
logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_pool_threshold_hits_20260516T201245Z.jsonl
```

Wynik threshold labeling:

- `threshold_rows=150`
- `OK=25`
- `NOK=42`
- `NONTARGET=69`
- `unresolved=14`
- `match_quality.tight<=2s=108`
- `match_quality.usable<=5s=136`
- unresolved reason: `entry_price_unavailable=14`

Nastepnie wygenerowano labels:

```bash
python3 scripts/gatekeeper_outcome_labeler.py \
  --decisions logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/shadow-burnin-v3-p32-replay-r10-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl \
  --threshold-hits logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_pool_threshold_hits_20260516T201245Z.jsonl \
  --output logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
```

Label summary:

- `decisions=150`
- `threshold_rows=150`
- `written=150`
- `label_valid=136`
- `hit_40_before_stop=25`
- `rug_or_early_death=42`

## P3.5 outcome quality

Komenda:

```bash
python3 scripts/v3_outcome_quality_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml \
  --outcome-labels logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --json
```

Wynik:

- `status=ok`
- `p3_5_status=outcome_quality_ready`
- `v3_rows=150`
- `known_outcome_rows=136`
- `outcome_label_coverage=0.906667`
- `outcome_label_counts.bad_entry=42`
- `outcome_label_counts.good_entry=25`
- `outcome_label_counts.neutral_entry=69`
- `outcome_label_counts.unknown=14`
- `effect_counts.v3_helped_avoided_bad_entry=42`
- `effect_counts.v3_hurt_blocked_good_entry=25`
- `effect_counts.v3_neutral_no_target=69`
- `effect_counts.inconclusive=14`
- `selected_good_entries=0`
- `selected_bad_entries=0`

Rozbicie po V3 reason:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: 15 avoided bad, 9 blocked good, 57 neutral, 13 inconclusive.
- `PENDING_V3_WAIT_EVIDENCE`: 25 avoided bad, 13 blocked good, 8 neutral, 1 inconclusive.
- `PENDING_V3_WAIT_SAMPLE`: 2 avoided bad, 3 blocked good, 4 neutral.

Rozbicie active -> V3:

- `REJECT->REJECT`: 15 bad, 9 good, 57 neutral, 13 unknown.
- `REJECT->PENDING`: 27 bad, 16 good, 12 neutral, 1 unknown.

## Active baseline validation

`scripts/gatekeeper_40pct_validation.py` na tych samych labelach zwrocil:

- `n=136`
- `selected=0`
- `coverage=0.0`
- `precision=null`

Interpretacja:

Aktywny baseline V2/V2.5 w tej probce nie dal zadnego BUY. Dlatego P3.5 r10 nie mierzy jeszcze
realnej precision aktywnych wejsc. Mierzy kontrfaktyczna jakosc blokowania V3 wzgledem outcome
labeli.

## Interpretacja biznesowa

R10 jest pierwsza wieksza probka, w ktorej V3 daje czytelny, sponsor-readable trade-off:

- V3 ochronil przed 42 zlymi wejściami.
- V3 zablokowal 25 dobrych wejsc.
- 69 decyzji bylo neutralnych.
- 14 decyzji pozostalo nierozstrzygnietych przez brak entry price.

To nie jest `FAILED`. To jest uzyteczny dowod, ze V3 ma realna funkcje ochronna, ale jest zbyt
konserwatywny i nadal generuje koszt alternatywny przez blokowanie dobrych wejsc.

To rowniez nie jest `APPROVED` dla P2. Nie ma podstaw do promocji, bo:

- V3 nie wybral zadnego dobrego wejscia jako BUY/EARLY_BUY,
- aktywny baseline rowniez nie mial BUY, wiec brakuje porownania precision na rzeczywistych wejsciach,
- probka nadal obejmuje jeden 30-minutowy primary-only run,
- `REJECT_V3_MANIPULATION_CONTRADICTION` i `PENDING_*` wymagaja dalszej kalibracji kosztu
  false-reject.

## Werdykt

`GO` dla dalszej P3.5 walidacji primary-only.

`NO-GO` dla P2 promotion.

Nastepny najbardziej sensowny krok: zebrac kolejne primary-only full replay runy i utrzymac ten sam
pomiar outcome quality, a nastepnie policzyc laczny trend `avoided_bad_entries` vs
`blocked_good_entries` na wiekszej probce.

## Weryfikacja

```bash
cargo test --workspace --no-run
bash scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml
python3 scripts/v3_full_replay_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml --strict --json
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml --json
python3 scripts/gatekeeper_outcome_labeler.py --decisions logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/shadow-burnin-v3-p32-replay-r10-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl --threshold-hits logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_pool_threshold_hits_20260516T201245Z.jsonl --output logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
python3 scripts/gatekeeper_40pct_validation.py --labels logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl --output logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_validation.json --bootstrap 200 --permutations 200
python3 scripts/v3_outcome_quality_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml --outcome-labels logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl --json
```
