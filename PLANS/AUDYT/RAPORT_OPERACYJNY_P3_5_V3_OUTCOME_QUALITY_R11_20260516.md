# Raport operacyjny P3.5 V3 Outcome Quality r11 primary-only

Data: 2026-05-16
Namespace: `shadow-burnin-v3-p32-replay-r11-primary-only`
Config: `configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml`
Status: `OUTCOME-QUALITY-READY / P2-NO-GO`

## Cel

R11 byl dluzszym primary-only runem P3.5 po r10.

Celem nie byla promocja V3, tuning progow ani wlaczenie FSC. Celem bylo powiekszenie probki
full replay evidence oraz policzenie outcome quality w tym samym jezyku co r10:

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

FSC pozostaje zde-scope'owany zgodnie z ADR-0130. `fsc=degraded` w rows R11 nie jest traktowany jako
negatywny sygnal decyzyjny ani jako dowod full-chain funding coverage.

## Przebieg

R11 byl uruchomiony w `tmux` bez timeoutu, zgodnie z decyzja operacyjna o dluzszym zbieraniu danych.
Run zakonczono manualnie po okolo dwoch godzinach pracy. Po zakonczeniu:

- brak aktywnego `ghost-launcher` / `cargo run`,
- brak aktywnej sesji `tmux`,
- artefakty R11 pozostaly w namespace `shadow-burnin-v3-p32-replay-r11-primary-only`.

## Full replay

Komenda:

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml \
  --strict \
  --json
```

Wynik:

- `status=ok`
- `replay_status=full_replay_ok`
- `total_rows=447`
- `v3_rows=447`
- `bad_rows=0`
- `status_counts.full_replay_ok=447`

Interpretacja:

R11 jest poprawnym full replay proof dla 447 V3 rows. Ten etap nie jest hash-only.

## Shadow report

Komenda:

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml \
  --json
```

Wynik:

- `status=ok`
- `replay_status=full`
- `raw_rows=447`
- `deduped_rows=447`
- `v3_rows=447`
- `duplicate_rows_removed=0`
- `full_snapshot_payload_rows=447`
- `hash_only_rows=0`
- `v3_policy_config_hash.coverage=1.0`
- `v3_feature_snapshot_hash.coverage=1.0`
- `policy_hash_unique_count=1`
- `snapshot_hash_unique_count=447`
- `stale_against_config=false`

Rozbicie active -> V3:

- `REJECT->REJECT=291`
- `REJECT->PENDING=156`

V3 reason codes:

- `REJECT_V3_MANIPULATION_CONTRADICTION=291`
- `PENDING_V3_WAIT_EVIDENCE=134`
- `PENDING_V3_WAIT_SAMPLE=22`

Confidence caps:

- `hard_risk=291`
- `insufficient_evidence=156`

## Coverage audit

Plik:

```text
logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/seer_runtime_coverage_audit.jsonl
```

Wynik:

- `coverage_rows=2487`
- `audit_status.ok=2479`
- `audit_status.rpc_error=8`
- brak wykrytych naruszen typu `emitted_without_rx` albo `runtime_accepted_without_emitted`

Interpretacja:

Coverage audit nie blokuje R11 jako evidence run. Osiem `rpc_error` wpisow traktujemy jako
infrastrukturalny residual, nie jako dowod naruszenia event routing contract.

## Outcome labels

Decision log:

```text
logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/shadow-burnin-v3-p32-replay-r11-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl
```

Threshold hits wygenerowano przez:

```bash
python3 logs/decisions.json/rollout/shadow-burnin/decisions/fetch_pool_price_at_30s.py \
  logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/shadow-burnin-v3-p32-replay-r11-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl \
  --output logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_pool_threshold_hits.jsonl \
  --checkpoint logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_pool_threshold_hits.checkpoint.jsonl \
  --workers 4 \
  --rps 20
```

RPC:

```text
https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa... (config.toml)
```

Finalny output:

```text
logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_pool_threshold_hits_20260516T231727Z.jsonl
```

Wynik threshold labeling:

- `threshold_rows=447`
- `OK=91`
- `NOK=84`
- `NONTARGET=212`
- `unresolved=60`
- `match_quality.tight<=2s=316`
- `match_quality.usable<=5s=387`
- unresolved reason: `entry_price_unavailable=60`

Nastepnie wygenerowano labels:

```bash
python3 scripts/gatekeeper_outcome_labeler.py \
  --decisions logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/shadow-burnin-v3-p32-replay-r11-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl \
  --threshold-hits logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_pool_threshold_hits_20260516T231727Z.jsonl \
  --output logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
```

Label summary:

- `decisions=447`
- `threshold_rows=447`
- `written=447`
- `label_valid=387`
- `hit_40_before_stop=91`
- `rug_or_early_death=84`

## P3.5 outcome quality

Komenda:

```bash
python3 scripts/v3_outcome_quality_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml \
  --outcome-labels logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --json
```

Wynik:

- `status=ok`
- `p3_5_status=outcome_quality_ready`
- `v3_rows=447`
- `known_outcome_rows=387`
- `outcome_label_coverage=0.865772`
- `outcome_label_counts.bad_entry=84`
- `outcome_label_counts.good_entry=91`
- `outcome_label_counts.neutral_entry=212`
- `outcome_label_counts.unknown=60`
- `effect_counts.v3_helped_avoided_bad_entry=84`
- `effect_counts.v3_hurt_blocked_good_entry=91`
- `effect_counts.v3_neutral_no_target=212`
- `effect_counts.inconclusive=60`
- `selected_good_entries=0`
- `selected_bad_entries=0`

Rozbicie po V3 reason:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: 40 avoided bad, 39 blocked good, 161 neutral, 51 inconclusive.
- `PENDING_V3_WAIT_EVIDENCE`: 40 avoided bad, 49 blocked good, 40 neutral, 5 inconclusive.
- `PENDING_V3_WAIT_SAMPLE`: 4 avoided bad, 3 blocked good, 11 neutral, 4 inconclusive.

Rozbicie active -> V3:

- `REJECT->REJECT`: 40 bad, 39 good, 161 neutral, 51 unknown.
- `REJECT->PENDING`: 44 bad, 52 good, 51 neutral, 9 unknown.

Najwieksze aktywne powody REJECT w R11:

- `REJECT_PDD_ENTRY_DRIFT=260`
- `REJECT_PDD_WHALE=130`
- `REJECT_PDD_FLASH_CRASH=31`
- `REJECT_PDD_SPIKE=8`
- `HARD_FAIL_MARKET_CAP=7`
- `REJECT_PDD_RAMPING=6`
- `REJECT_LOW_TRAJECTORY=5`

## Active baseline validation

`scripts/gatekeeper_40pct_validation.py` na tych samych labelach zwrocil:

- `n=387`
- `selected=0`
- `coverage=0.0`
- `precision=null`

Interpretacja:

Aktywny baseline V2/V2.5 w tej probce nie dal zadnego BUY. Dlatego R11 nadal nie mierzy precision
aktywnych wejsc. Mierzy kontrfaktyczna jakosc blokowania V3 wzgledem outcome labeli.

## Interpretacja biznesowa

R11 powieksza probke z r10 z 150 do 447 decyzji, ale wynik jakosciowy jest gorszy niz w r10:

- V3 ochronil przed 84 zlymi wejsciami.
- V3 zablokowal 91 dobrych wejsc.
- 212 decyzji bylo neutralnych.
- 60 decyzji pozostalo nierozstrzygnietych przez brak entry price.

Relacja ochrony do kosztu alternatywnego wynosi `84/91 = 0.92`. To znaczy, ze w tej probce V3
zablokowal wiecej dobrych okazji niz zlych wejsc. To jest mocny sygnal, ze obecny profil V3 jest
zbyt konserwatywny i nie nadaje sie do promocji.

R11 nie oznacza, ze kierunek V3 jest bezwartosciowy. Full replay dziala, outcome labeling dziala, a
system daje policzalne trade-offy. Natomiast jako polityka decyzyjna obecny profil V3 wymaga dalszej
kalibracji shadow-only, bo nie wykazuje jeszcze korzystnej relacji `avoided_bad_entries` do
`blocked_good_entries`.

## Porownanie z r10

- r10: `42` avoided bad vs `25` blocked good, ratio `1.68`, probka 150 rows.
- r11: `84` avoided bad vs `91` blocked good, ratio `0.92`, probka 447 rows.

R11 jest wieksza i przez to wazniejsza probka. Wynik r11 oslabia optymistyczna interpretacje r10 i
przesuwa rekomendacje z "kontynuowac walidacje" do "kontynuowac, ale z przygotowaniem planu
kalibracji shadow-only przed kolejnym runem porownawczym".

## Werdykt

`GO` dla dalszej P3.5/P3.6 analizy i kalibracji shadow-only.

`NO-GO` dla P2 promotion.

Nie nalezy teraz promowac V3 ani zmieniac active V2/V2.5. Najbardziej sensowny nastepny krok to
P3.6 shadow-only calibration plan oparty o R10+R11, z naciskiem na:

- `PENDING_V3_WAIT_EVIDENCE`, bo w R11 blokuje 49 dobrych wejsc przy 40 avoided bad,
- rozdzielenie `REJECT_V3_MANIPULATION_CONTRADICTION` na subtypy analityczne,
- traktowanie PENDING jako effective block, dopoki nie prowadzi do pozniejszego BUY,
- utrzymanie full replay/outcome labels jako gate dla kazdej zmiany progow.

## Weryfikacja

```bash
python3 scripts/v3_full_replay_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml --strict --json
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml --json
python3 logs/decisions.json/rollout/shadow-burnin/decisions/fetch_pool_price_at_30s.py logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/shadow-burnin-v3-p32-replay-r11-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl --output logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_pool_threshold_hits.jsonl --checkpoint logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_pool_threshold_hits.checkpoint.jsonl --workers 4 --rps 20
python3 scripts/gatekeeper_outcome_labeler.py --decisions logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/shadow-burnin-v3-p32-replay-r11-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl --threshold-hits logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_pool_threshold_hits_20260516T231727Z.jsonl --output logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
python3 scripts/gatekeeper_40pct_validation.py --labels logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl --output logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_validation.json --bootstrap 200 --permutations 200
python3 scripts/v3_outcome_quality_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml --outcome-labels logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl --json
```
