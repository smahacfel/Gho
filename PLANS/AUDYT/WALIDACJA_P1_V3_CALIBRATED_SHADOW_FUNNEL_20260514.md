# Walidacja P1 V3 Calibrated Shadow Funnel - 2026-05-14

## Status po audycie i remediacji 2026-05-14

Pierwotna realizacja P1 zostala odrzucona w audycie jako niezgodna z kontraktem
P1. Dzialaly addytywne pola JSONL i raport, ale naruszone byly trzy wazne
granice:

- aktywny config V2/V2.5 zostal zmieniony (`min_market_cap_sol=30.0` zamiast
  poprzedniego `41.0`),
- model `GatekeeperV3Config` nie obejmowal pelnego calibrated funnel
  (`early/normal/extended`, `evidence_requirements`, `confidence_caps`,
  `component_weights`), a wagi/capy scoringu pozostawaly hardcoded w kodzie,
- `v3_feature_snapshot_hash` nie obejmowal `v3_materialization_version` i
  mieszal stabilne cechy z identyfikatorem sesji.

Remediacja wykonana w working tree:

- przywrocono aktywny `gatekeeper_v2.min_market_cap_sol = 41.0`,
- rozbudowano `GatekeeperV3Config` do profili stage i sekcji calibrated funnel,
- evaluator V3 uzywa wag/capow/progow z configu zamiast hardcoded scoringu,
- `v3_feature_snapshot_hash` obejmuje `materialization_version` i nie obejmuje
  `session_id`,
- actionability uzywa `group + status + stage + config`,
- actionability uzywa tego samego profilu `early/normal/extended`, ktory
  wybiera evaluator dla danego `deadline_elapsed + observation_duration_ms`,
- raport liczy diagnostyke konfliktowych duplikatow przed deduplikacja,
- raport zwraca `stale_artifacts`, gdy znaleziony decision log jest starszy niz
  rollout config albo wskazany `ghost_brain_config.toml`.

Aktualny status po rerunie 2026-05-15: `APPROVED` dla formalnego domkniecia
runtime evidence P1. Clean rerun po remediacji wyprodukowal swieze decision rows:
`status=ok`, `v3_rows=86`, `artifact_freshness.stale_against_config=false`,
`replay_status=hash_only`, hash coverage `1.0`.

Raport `status=ok`, `v3_rows=29`, hash coverage `1.0` i
`replay_status=hash_only` opisany nizej jest historycznym evidence sprzed tej
remediacji. Nie nalezy go traktowac jako dowodu, ze zremediowane actionability i
nowy snapshot hash zostaly juz potwierdzone na swiezych runtime decyzjach.

Po follow-up remediacji 2026-05-15 skrypt raportujacy oznacza historyczny plik
29 rows jako `status=stale_artifacts`, poniewaz selected decision log jest
starszy niz referencyjny `ghost_brain_config.toml`. Swiezy run 86 rows jest
obecnym baseline P1.

## P1 Closure Note - 2026-05-15

P1 zostaje zamrozony jako audytowalny baseline na commit:

- `0f3639c Close V3 P1 remediation and runtime evidence`

Kanoniczny raport operacyjny:

- `PLANS/AUDYT/RAPORT_OPERACYJNY_P1_V3_SHADOW_RERUN_20260515.md`

Wynik runtime:

- `status=ok`
- `v3_rows=86`
- `artifact_freshness.stale_against_config=false`
- `replay_status=hash_only`
- `v3_policy_config_hash.coverage=1.0`
- `v3_feature_snapshot_hash.coverage=1.0`
- `pre_dedupe_conflicts.conflict_groups=0`

Residual:

- `funding_lane_full_chain` nadal lapal Yellowstone
  `ResourceExhausted: Concurrent Yellowstone Geyser stream limit reached`.
- Residual jest infrastrukturalnym follow-upem lane pomocniczego i nie blokuje
  domkniecia P1, poniewaz primary `v25_shadow` path wygenerowal swieze decision
  rows oraz coverage audit.

Nastepny etap:

- przejsc do P3 Calibration Gate,
- nie rozpoczynac P2 promotion,
- traktowac `hash_only` jako ograniczenie replay, nie jako full replay OK.

## Zakres

Zrealizowano tylko P1.1-P1.5 jako addytywny V3 sidecar.

In scope:

- konfiguracja `GatekeeperV3Config` z backward-compatible defaultami,
- przepiecie ewaluatora V3 na `GatekeeperV3Config`,
- deterministyczne hashe `v3_policy_config_hash` i `v3_feature_snapshot_hash`,
- addytywne pola JSONL `v3_*`,
- rozszerzenie `scripts/v3_shadow_report.py`,
- namespace artefaktow `shadow-burnin-v3-p1`,
- targeted tests i clean-start shadow rerun.

Out of scope:

- aktywna polityka V2/V2.5,
- `GatekeeperBuffer::evaluate_from_features()`,
- IWIM,
- live sender / execution,
- zmiana progow policy,
- `decision_plane = "v3_shadow"`,
- bump schemy JSONL.

## Commity

- `ede4bdc Add Gatekeeper V3 shadow config`
- `a7a5d64 Wire Gatekeeper V3 shadow sidecar`
- `a29c87c Expand Gatekeeper V3 shadow report`
- `5fb3c63 Use Gatekeeper V3 P1 shadow artifact namespace`

## Pliki i kontrakty

- `ghost-brain/src/config/gatekeeper_v3_config.rs` dodaje `GatekeeperV3Config` z defaultami: `enabled=false`, `shadow_emit_enabled=false`, `policy_version=1`, `materialization_version=1`, `promotion.enabled=false`.
- `ghost-brain/src/config/ghost_brain_config.rs` dodaje `#[serde(default)] pub gatekeeper_v3: GatekeeperV3Config`, wiec stare TOML bez `[gatekeeper_v3]` nadal sie laduja.
- `ghost-brain/ghost_brain_config.toml` jawnie ustawia `[gatekeeper_v3] shadow_emit_enabled=true`; `enabled=false` pozostaje poza semantyka P1.
- `ghost-launcher/src/components/gatekeeper_v3.rs` uzywa `&GatekeeperV3Config`; wrapper kompatybilny pozostaje adapterem testowym.
- `ghost-brain/src/oracle/decision_logger.rs` dostal tylko addytywne `Option<T>` pola V3 z `#[serde(default, skip_serializing_if = "Option::is_none")]`; stare JSONL bez P1 fields nadal sie parsuje.
- `configs/rollout/shadow-burnin.toml` kieruje artefakty do `shadow-burnin-v3-p1`; progi policy i `payer_strategy` nie zostaly zmienione.

## Weryfikacja targeted po pierwotnej realizacji

Przeszly w pierwotnej realizacji:

```bash
cargo test -p ghost-core materialized
cargo test -p ghost-core feature_builder
cargo test -p ghost-brain gatekeeper_v3_config
cargo test -p ghost-brain decision_logger
cargo test -p ghost-brain reason_code
cargo test -p ghost-launcher gatekeeper_v3
cargo test -p ghost-launcher v3_shadow
python3 -m unittest scripts/test_v3_shadow_report.py
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
```

Przed rerunem raport zwracal `status=no_rows`, co bylo oczekiwane dla pustego namespace P1.

## Weryfikacja targeted po remediacji

Przeszly:

```bash
cargo test -p ghost-core materialized
cargo test -p ghost-core feature_builder
cargo test -p ghost-brain gatekeeper_v3_config
cargo test -p ghost-brain decision_logger
cargo test -p ghost-brain reason_code
cargo test -p ghost-launcher gatekeeper_v3
cargo test -p ghost-launcher v3_shadow
python3 -m unittest scripts/test_v3_shadow_report.py -v
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
git diff --check
```

Zakres testow po remediacji potwierdza:

- production TOML laduje sie i waliduje z `gatekeeper_v2.min_market_cap_sol = 41.0`,
- V3 config hash reaguje na zmiany sekcji calibrated funnel,
- V3 feature snapshot hash obejmuje `materialization_version` i nie dryfuje od
  `session_id`,
- zmiana `component_weights` w configu zmienia opportunity score bez zmian kodu,
- zdegradowane `manipulation_contradiction` blokuje actionability stage `risk`,
- raport pokazuje konfliktowe duplikaty przed deduplikacja.
- raport oznacza historyczne artefakty jako `stale_artifacts`, gdy nie sa
  swiezsze od configu.
- `early_window_ms` wybiera profil `early` przed oknem early, `normal` po nim i
  `extended` po deadline.

## Clean P1 Rerun po pierwotnej realizacji

Uruchomiono:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin.toml
```

Start runtime potwierdzil:

- `execution_mode=Shadow`
- `entry_mode=shadow_only`
- `Gatekeeper V3 sidecar config: enabled=false shadow_emit=true policy_version=1 materialization_version=1`
- artifact namespace `shadow-burnin-v3-p1`

Proces zostal zatrzymany kontrolowanym `SIGTERM` po uzyskaniu wystarczajacego P1 evidence i po pojawieniu sie lawiny `Transport channel disconnected` po `SIGINT`. To nie byl naturalny 30-minutowy timeout.

Glowne artefakty:

- `/root/Gho/logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl`
- `/root/Gho/logs/rollout/shadow-burnin-v3-p1/decisions/seer_runtime_coverage_audit.jsonl`
- `/root/Gho/datasets/events/shadow-burnin-v3-p1/`
- `/root/Gho/logs/rollout/shadow-burnin-v3-p1/system.log.2026-05-14`
- `/root/Gho/logs/rollout/shadow-burnin-v3-p1/oracle.log.2026-05-14`

## Clean P1 Reruny po remediacji

Pierwsze podejscie po remediacji uruchomiono komenda:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin.toml
```

Wynik pierwszego podejscia:

- proces zakonczyl sie kodem `124`, czyli naturalnym `timeout 30m`,
- start runtime potwierdzil `execution_mode=Shadow` i `entry_mode=shadow_only`,
- V3 sidecar wystartowal jako `enabled=false shadow_emit=true
  policy_version=1 materialization_version=1`,
- DecisionLogger kierowal logi do namespace
  `/root/Gho/logs/rollout/shadow-burnin-v3-p1/decisions`,
- primary Yellowstone stream nie wystartowal z powodu
  `ResourceExhausted: Concurrent Yellowstone Geyser stream limit reached`,
- GatekeeperCommitLoop raportowal `commits=0, active_buffers=0`,
- po zakonczeniu procesu nie pozostal dzialajacy `ghost-launcher`.

To pierwsze podejscie potwierdzilo shadow-only start i naturalny timeout, ale
nie domykalo evidence decyzji po remediacji. Brak nowych decision rows byl
zewnetrznym blokerem infrastruktury, nie pozytywnym dowodem semantyki P1.

Finalny clean rerun po zwolnieniu slotu Yellowstone zostal wykonany 2026-05-15
i opisany w:

- `PLANS/AUDYT/RAPORT_OPERACYJNY_P1_V3_SHADOW_RERUN_20260515.md`

Wynik finalnego rerunu:

- proces zakonczyl sie naturalnie po `timeout 30m`,
- swiezy `v25_shadow` decision log zawiera `86` V3 rows,
- `status=ok`,
- `artifact_freshness.stale_against_config=false`,
- `replay_status=hash_only`,
- `v3_policy_config_hash.coverage=1.0`,
- `v3_feature_snapshot_hash.coverage=1.0`,
- `pre_dedupe_conflicts.conflict_groups=0`,
- residual `funding_lane_full_chain` / Yellowstone `ResourceExhausted` pozostal
  jako infra follow-up, ale nie zablokowal primary P1 evidence.

## Finalny Raport P1 z pierwotnego evidence

Komenda:

```bash
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
```

Wynik z historycznego rerunu przed follow-up guardem:

- `status=ok`
- `raw_rows=29`
- `deduped_rows=29`
- `v3_rows=29`
- `bad_rows=0`
- `no_v3_rows=0`
- `duplicate_rows_removed=0`
- `v3_policy_config_hash.coverage=1.0`
- `v3_feature_snapshot_hash.coverage=1.0`
- `policy_hash_unique_count=1`
- `snapshot_hash_unique_count=29`
- `snapshot_uniqueness.duplicate_row_count=0`
- `replay_status=hash_only`

Wynik po follow-up guardzie 2026-05-15 na tym samym historycznym pliku:

- `status=stale_artifacts`
- `raw_rows=29`
- `v3_rows=29`
- `artifact_freshness.stale_against_config=true`
- `replay_status=hash_only`

To nie zmienia analityki historycznych rows, ale zmienia status raportu tak, aby
nie wygladal jak swiezy dowod po remediacji.

Hash/config matrix:

- V2/V2.5 routing `config_hash`: `05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68`
- V3 policy hash: `cbac13ab21b1a09d4d1e47b1656f6593815e728d738c2e130fbc757402e3861d`
- count: `29`

`replay_status=hash_only` jest poprawnym wynikiem dla P1, bo JSONL ma stabilne snapshot hashes, ale nie zawiera pelnego payloadu `MaterializedFeatureSet` do pelnego replay.

## Interpretacja PENDING / REJECT vs P0

P1 poprawia interpretowalnosc wzgledem P0, bo rozbija aktywne V2/V2.5 `REJECT` na osobna warstwe V3:

- active `REJECT` -> V3 `REJECT`: `20`
- active `REJECT` -> V3 `PENDING`: `9`

V3 reason distribution:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: `20`
- `PENDING_V3_WAIT_EVIDENCE`: `7`
- `PENDING_V3_WAIT_SAMPLE`: `2`

Stage distribution:

- `RISK`: `20`
- `EVIDENCE`: `9`

Component buckets pokazuja, ze final confidence zostalo wyzerowane we wszystkich 29 wierszach, ale z dwoch roznych powodow:

- `hard_risk`: `20`
- `insufficient_evidence`: `9`

To daje lepsza diagnostyke niz sam P0: `REJECT` nie jest juz jednolita kategoria interpretacyjna. P1 rozroznia przypadki, ktore V3 odrzuca przez risk/manipulation contradiction, od przypadkow, ktore V3 klasyfikuje jako brak wystarczajacej probki/evidence i trzyma w `PENDING`.

Actionability:

- stages `opportunity` i `risk`: `actionable=29`
- stages `evidence` i `confidence`: `not_actionable=29`
- `organic_broadening`, `pdd_sequence`, `tx_segments`: `actionable=18`, `wait_sample=11`
- `fsc`, `sybil`, `execution`, `manipulation_contradiction`: degraded/shadow-only dla 29 wierszy

Wniosek: P1 daje uzyteczna interpretacje PENDING/REJECT dla shadow funnel, ale nie jest dowodem pelnego replay ani dowodem wykonania transakcji. Execution w raporcie ma `missing=29`, `success_count=0`, zgodnie z zasada, ze submit/no_dispatch/missing nie sa sukcesem.

## Acceptance po remediacji

- Stare TOML bez `[gatekeeper_v3]`: pokryte defaultem i testem `gatekeeper_v3_config`.
- Stare JSONL bez P1 fields: zachowane przez addytywne `Option<T>` pola V3.
- Aktywny config V2/V2.5: przywrocony `min_market_cap_sol=41.0`; test
  ladowania production TOML chroni przed ponownym dryfem.
- V3 sidecar-only: `enabled=false`, `shadow_emit_enabled=true`; remediacja nie
  dotyka IWIM ani live sender.
- V3 calibrated funnel: progi, evidence requirements, confidence caps i
  component weights sa w `GatekeeperV3Config`.
- Snapshot hash: obejmuje `materialization_version` i nie obejmuje `session_id`.
- P1 report: swiezy baseline ma `v3_rows=86`, hash coverage `1.0`,
  `replay_status=hash_only`, `status=ok`, `stale_against_config=false`.
  Historyczne 29 rows pozostaja oznaczone jako `status=stale_artifacts`.

## Residual Risk

- Historyczny rerun nie zakonczyl sie naturalnym 30-minutowym timeoutem; zostal
  zatrzymany po uzyskaniu wystarczajacego P1 evidence.
- Rerun po remediacji zakonczyl sie naturalnym timeoutem i wytworzyl 86 swiezych
  `v25_shadow` rows.
- `funding_lane_full_chain` nadal raportowal Yellowstone `ResourceExhausted`,
  ale nie zablokowal glownego shadow evidence.
- `hash_only` nie pozwala odtworzyc pelnego `MaterializedFeatureSet`; potwierdza deterministyczne porownanie snapshot hash, nie full replay.
- Brak execution success jest oczekiwany w shadow-only, ale oznacza, ze P1 waliduje interpretacje decyzji, nie lifecycle wykonania.

## Werdykt po remediacji

`APPROVED` dla formalnego domkniecia P1 runtime evidence package.

P1 jest zamrozony jako baseline do P3 Calibration Gate. Dalsze prace nie powinny
promowac V3 do active path bez osobnego P2 ADR i pozytywnego P3 certification.
