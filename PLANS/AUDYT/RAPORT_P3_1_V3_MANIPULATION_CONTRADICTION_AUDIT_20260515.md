# Raport P3.1 V3 Manipulation Contradiction Audit - 2026-05-15

## Decyzja

P3.1 targeted audit dla `REJECT_V3_MANIPULATION_CONTRADICTION` konczy sie
statusem:

```text
p3_1_status=keep_blocked_needs_full_replay
promotion_ready=false
no_p2_promotion=true
```

Bucket nie wyglada jak pusty agregat degradacji: w primary P1 baseline `70/70`
rekordow ma co najmniej jeden hard-risk trigger progowy. Jednoczesnie `68/70`
rekordow ma zdegradowane evidence/actionability dla
`manipulation_contradiction`, wiec bucket nie jest gotowy do promocji ani ADR
bez pelnego replay albo dalszej manualnej walidacji.

## Zakres

In scope:

- offline audit tylko z JSONL,
- primary P1 baseline `86` rows,
- historical P1 `29` rows,
- older repair run `141` rows jako porownanie pomocnicze,
- reason/cap/actionability/component-score breakdown,
- przykladowe rekordy per hard-risk subtype,
- korelacja z active V2/V2.5 `reason_code`.

Out of scope:

- P2 promotion,
- zmiana scoringu,
- zmiana progow,
- zmiana active V2/V2.5/IWIM/execution,
- traktowanie `hash_only` jako full replay.

## Narzedzie

Dodano helper offline:

```bash
python3 scripts/v3_manipulation_contradiction_audit.py \
  --config configs/rollout/shadow-burnin.toml \
  --compare-decisions-log logs/rollout/shadow-burnin-v3-p1.20260515T111441Z.pre-rerun/decisions/shadow-burnin-v3-p1/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl \
  --compare-decisions-log logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl \
  --json
```

Test:

```bash
python3 -m unittest scripts/test_v3_manipulation_contradiction_audit.py -v
```

Skrypt czyta tylko decision JSONL. Nie zmienia runtime, configu, progow ani
active policy.

## Inputy

Primary P1 baseline:

- `logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`
- `v3_rows=86`
- target bucket rows: `70`
- target share: `0.813953`
- `decision_plane=v25_shadow` dla `70/70`
- missing `ab_record_id`: `0`
- V3 policy hash: `9b55a78eb05943e6bd89b28d7f78ef9eac714346476a86553877bce47d07ab1c`

Historical P1:

- rows: `29`
- target bucket rows: `20`
- target share: `0.689655`

Older repair run:

- rows: `141`
- target bucket rows: `96`
- target share: `0.680851`
- note: brak `v3_stage_thresholds`, wiec hard-risk trigger reconstruction jest
  oznaczona jako `thresholds_missing`.

## Primary Breakdown

### Clean vs Degraded

`manipulation_contradiction` evidence:

- `clean=2`
- `degraded=68`
- `other=0`

Actionability:

- `manipulation_contradiction.actionable=2`
- `manipulation_contradiction.not_actionable=68`
- `risk_stage.actionable=2`
- `risk_stage.not_actionable=68`

Wniosek: bucket jest prawie w calosci oparty o zdegradowany
`manipulation_contradiction` evidence path, mimo ze komponent risk ma
`ACTIONABLE=70` i confidence cap `hard_risk=70`.

### Feature Evidence

Primary target `70` rows:

- `manipulation.clean=70`
- `manipulation_contradiction.clean=2`, `degraded=68`
- `sybil.clean=3`, `degraded=67`
- `fsc.clean=5`, `degraded=65`
- `organic_broadening.clean=27`, `insufficient_sample=43`
- `pdd_sequence.clean=27`, `insufficient_sample=43`
- `tx_segments.clean=27`, `insufficient_sample=43`

Dominujace degraded reasons:

- `manipulation_contradiction_partial=68`
- `sybil_evidence_partial=67`
- `fsc_evidence_partial=65`
- `segment_sequence_partial=43`
- `pdd_sequence_partial=43`

## Hard-Risk Subtypes

Primary `70/70` ma hard-risk trigger coverage `1.0`.

Trigger counts:

- `dev_volume_ratio_threshold=59`
- `hhi_threshold=32`
- `top3_volume_pct_threshold=28`
- `bundle_suspicion_ratio_gt_hard_fail_same_ms_tx_ratio=4`

Trigger combinations:

- `dev_volume_ratio_threshold`: `29`
- `top3_volume_pct_threshold+hhi_threshold+dev_volume_ratio_threshold`: `20`
- `hhi_threshold+dev_volume_ratio_threshold`: `7`
- `top3_volume_pct_threshold`: `3`
- `hhi_threshold`: `3`
- `bundle_suspicion_ratio_gt_hard_fail_same_ms_tx_ratio`: `3`
- `top3_volume_pct_threshold+dev_volume_ratio_threshold`: `2`
- `top3_volume_pct_threshold+hhi_threshold`: `2`
- `bundle_suspicion_ratio_gt_hard_fail_same_ms_tx_ratio+top3_volume_pct_threshold+dev_volume_ratio_threshold`: `1`

Interpretacja:

- Najwiekszy realny driver to `dev_volume_ratio_threshold`, nie sama etykieta
  `timing_bundle_concentration`.
- `top3_volume_pct` i `hhi` czesto wspieraja ten sam bucket, ale rzadziej jako
  jedyny trigger.
- `bundle_suspicion_ratio_gt_hard_fail_same_ms_tx_ratio` jest rzadkie (`4/70`)
  i wymaga osobnego przegladu nazewnictwa/progu, bo uzywa tego samego limitu co
  `same_ms_tx_ratio` w obecnym V3 predicate.

## Manipulation Reasons

Logged manipulation reason counts:

- `timing_bundle_concentration=68`
- `early_top3_concentration=63`
- `fixed_size_or_ramping_pattern=14`
- `high_buy_pressure_with_high_top3=6`
- `momentum_without_broadening=1`

Boolean flags:

- `dev_has_sold=60`
- `sybil_evidence_degraded=67`
- `timing_bundle_concentration=68`
- `early_top3_concentration=63`
- `fixed_size_or_ramping_pattern=14`
- `high_buy_pressure_with_high_top3=6`
- `momentum_without_broadening=1`

`dev_has_sold` nie jest hard triggerem w obecnym profilu, bo
`reject_on_dev_sell=false`. Wystepuje jednak razem z `dev_volume_ratio_threshold`
i wzmacnia interpretacje ryzyka dev concentration.

## Component Scores

Primary target `70` rows:

- `risk_statuses.ACTIONABLE=70`
- `risk_penalty_buckets.0_75_to_1_00=70`
- `confidence_cap_reasons.hard_risk=70`
- `final_confidence_buckets.0=70`
- `opportunity_statuses.UNAVAILABLE=70`

Raw confidence / opportunity score przed hard-risk cap:

- `0_25_to_0_50=50`
- `0_50_to_0_75=19`
- `0_75_to_1_00=1`

Wniosek: V3 nie odrzuca tych rows przez slaby opportunity score. Odrzucenie
pochodzi z risk cap, ktory zeruje final confidence mimo czesto dodatniego raw
opportunity.

## Active V2/V2.5 Correlation

Primary active reason codes dla tych samych `70` rows:

- `REJECT_PDD_ENTRY_DRIFT=33`
- `REJECT_PDD_WHALE=33`
- `HARD_FAIL_MARKET_CAP=2`
- `REJECT_PDD_FLASH_CRASH=1`
- `REJECT_LOW_TRAJECTORY=1`

Historical P1:

- `REJECT_PDD_WHALE=11`
- `REJECT_PDD_ENTRY_DRIFT=8`
- `REJECT_PDD_SPIKE=1`

Older repair run:

- `REJECT_PDD_ENTRY_DRIFT=52`
- `REJECT_PDD_WHALE=35`
- `REJECT_PDD_RAMPING=6`
- pozostale pojedyncze: `REJECT_PDD_SPIKE`, `REJECT_LOW_TRAJECTORY`,
  `REJECT_PDD_FLASH_CRASH`

Interpretacja: V3 bucket nie jest losowo oderwany od aktywnego V2/V2.5 reject
path. Najczesciej pokrywa sie z PDD drift/whale, czyli z aktywnymi rejectami
zwiazanymi z ruchem wejscia i koncentracja. Nie daje to jeszcze dowodu
true-negative, ale wspiera hipoteze, ze bucket lapie realny risk family.

## Numeric Snapshot

Primary target `70` rows:

- `dev_volume_ratio`: avg `0.350281`, p50 `0.347526`, p90 `0.600935`, max `0.822213`
- `top3_volume_pct`: avg `0.661558`, p50 `0.677989`, p90 `0.850795`, max `0.959137`
- `hhi`: avg `0.095892`, p50 `0.092593`, p90 `0.132653`, max `0.356009`
- `bundle_suspicion_ratio`: avg `0.402830`, p50 `0.406250`, p90 `0.500000`, max `0.800000`
- `same_ms_tx_ratio`: avg `0.128179`, p50 `0.115385`, p90 `0.250000`, max `0.533333`
- `contradiction_score`: avg `0.361905`, p50 `0.333333`, p90 `0.500000`, max `0.666667`

Wniosek: target bucket ma mierzalne koncentracje dev/top3/hhi, ale nie wszystkie
metryki sa rownie mocne. `same_ms_tx_ratio` nie jest dominujacym hard triggerem w
primary baseline.

## Przyklady Per Subtype

`dev_volume_ratio_threshold`:

- pool `GQZgCGckkU5xDtwxjVj6E7aENi4Tzwpz1xr5b8MNSZHz`
- active `REJECT_PDD_ENTRY_DRIFT`
- `dev_volume_ratio=0.284658`, `top3_volume_pct=0.488794`, `hhi=0.059313`
- evidence `degraded`, actionability `not_actionable`

`top3_volume_pct_threshold+hhi_threshold+dev_volume_ratio_threshold`:

- pool `GFqFb8VyASZZbFvu9e1XKKSveRZHE5MEFvqyw8HVpQmT`
- active `REJECT_PDD_WHALE`
- `dev_volume_ratio=0.246339`, `top3_volume_pct=0.759780`, `hhi=0.128889`
- evidence `degraded`, actionability `not_actionable`

`bundle_suspicion_ratio_gt_hard_fail_same_ms_tx_ratio`:

- pool `HFqFfLoBpD9kDDdVDPZQgVKAmBBMEsnXhRXUsuvT7pVQ`
- active `REJECT_PDD_ENTRY_DRIFT`
- `bundle_suspicion_ratio=0.666667`, `same_ms_tx_ratio=0.250000`
- evidence `degraded`, actionability `not_actionable`

`hhi_threshold`:

- pool `BEbuLs2opUdumz3HnH7EA5J6TvD1wjBM9wvFsyX1ELVQ`
- active `REJECT_PDD_ENTRY_DRIFT`
- `hhi=0.120000`, `dev_volume_ratio=0.212044`, `top3_volume_pct=0.624049`
- evidence `degraded`, actionability `not_actionable`

## Cross-Run Stability

Target rows:

- primary P1: `70/86 = 0.813953`
- historical P1: `20/29 = 0.689655`
- older repair run: `96/141 = 0.680851`

Dominant active reason:

- primary: `REJECT_PDD_WHALE=33` and `REJECT_PDD_ENTRY_DRIFT=33`
- historical P1: `REJECT_PDD_WHALE=11`
- older repair run: `REJECT_PDD_ENTRY_DRIFT=52`

Dominant hard trigger:

- primary: `dev_volume_ratio_threshold=59`
- historical P1: `dev_volume_ratio_threshold=17`
- older repair run: `thresholds_missing=96`, so hard-trigger reconstruction is
  not available for this older artifact.

Interpretacja: bucket jest stabilnie dominujacy w trzech datasetach, ale tylko
primary i historical P1 maja wystarczajace threshold payloady do trigger
reconstruction. Starszy repair run sluzy tylko jako reason/evidence distribution
comparison.

## Odpowiedz na pytanie P3.1

Czy `REJECT_V3_MANIPULATION_CONTRADICTION` jest realnym hard-risk sygnalem czy
zbyt agresywnym agregatem degradacji?

Wynik:

- Jest realny hard-risk candidate: `70/70` primary rows ma hard-risk trigger, a
  `59/70` przekracza `dev_volume_ratio_threshold`.
- Jest zbyt szeroki jako pojedynczy bucket promocyjny: `68/70` rows ma
  `manipulation_contradiction` evidence zdegradowane i `not_actionable`, a
  logged reasons mieszaja timing/top3/dev concentration pod jedna etykieta.
- Nie jest promotion-ready: brak full replay, brak outcome labels, brak
  kontrfaktycznego recompute.

Decyzja:

```text
keep blocked
needs full replay
candidate for future ADR only after subtype split / manual audit
```

## Nastepne Kroki

1. P3.2 Full Replay Payload Design: zaprojektowac pelny replay
   `MaterializedFeatureSet`, zeby wyjsc z `hash_only`.
2. P3.3 Multi-run Stability Collection: zebrac kolejne swieze runy na tym samym
   `v3_policy_config_hash`.
3. Przed jakimkolwiek ADR rozdzielic lub osobno oceniajac subtypy:
   `dev_volume_ratio_threshold`, `top3_volume_pct_threshold`, `hhi_threshold`,
   `bundle_suspicion_ratio_gt_hard_fail_same_ms_tx_ratio`.

## Weryfikacja

Przeszlo:

```bash
python3 -m unittest scripts/test_v3_manipulation_contradiction_audit.py -v
python3 scripts/v3_manipulation_contradiction_audit.py --config configs/rollout/shadow-burnin.toml --compare-decisions-log logs/rollout/shadow-burnin-v3-p1.20260515T111441Z.pre-rerun/decisions/shadow-burnin-v3-p1/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl --compare-decisions-log logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl
```

Runtime contract:

- `active_policy_changed=false`
- `promotion_activated=false`
- `decision_plane_v3_shadow_created=false`
