# ADR-8D: HET Position Manager V2 PR A — observe-only trajectory, policy i executable anchor

Status: `IMPLEMENTED LOCALLY / PR A OBSERVE-ONLY / REVIEW READY / ONE UNRELATED BASELINE TEST FAILURE DOCUMENTED`

Typ: ADR-8D / aktywny shadow post-buy / HET-PM V2 PR A / evidence i kontrfaktyczna polityka

Data: 2026-07-16

Repo: `smahacfel/Gho`

Branch: `main` (lokalny worktree PR A; bez commit/push)

Base SHA: `18d94b0cc5a226496a5ac2bc616e7488a7f78d5d`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`, wyłącznie PR A.

Poziom ryzyka: `MEDIUM` — zmiana działa w aktywnym shadow post-buy runtime,
ale HET-PM V2 jest konstrukcyjnie observe-only. V1 pozostaje jedynym ownerem
proposal/apply/terminal/capacity, a live execution pozostaje wyłączone.

## 1. Problem i decyzja

Position Manager Lite V1 posiadał poprawny pojedynczy lifecycle owner, lazy
full-position quote, guarded apply, bounded recovery i canonical terminal
commit. Nie materializował jednak spójnego, nie-lookahead obrazu trajektorii,
executable peak anchora ani porównania hierarchicznej polityki HET-PM V2 z
rzeczywistą decyzją V1.

PR A wdraża następujący przepływ:

```text
existing MonitoredPosition + existing bounded SnapshotTimeline
  -> immutable PostBuySnapshotBundle (V1 base + V2 extras)
  -> pure V1 prequote
  -> pure V2 prequote
  -> pure peak-anchor request
  -> lokalny, bounded, deduplikowany quote plan
  -> precomputed V1/V2 comparison
  -> V1 authority tick i canonical terminal contract
  -> observer-only anchor apply, tylko gdy pozycja i guard nadal są aktualne
  -> fail-open het_pm_v2_observations_v1.jsonl
```

Decyzja architektoniczna: nie powstaje drugi `PositionStore`, drugi timeline,
drugi terminal writer ani drugi action owner. PR A nie posiada ścieżki, która
mogłaby konsumować wynik V2 do proposal/apply.

## 2. TrajectoryFeaturesV1

Nowy pure moduł `trajectory_v1.rs` projektuje wyłącznie dane dostępne do
timestampu newest sample. Kontrakt samplingowy jest jawny:

```text
latest_canonical_state_per_monitor_tick
online_non_lookahead_sampled_trajectory
```

Materializowane pola obejmują tylko:

- return 1500 ms;
- return 5 s;
- return 15 s;
- peak mark price i jego slot/timestamp;
- drawdown od peak;
- time since peak;
- peak giveback velocity;
- bounded quality/provenance: newest sample, distinct short-window slots,
  state-update delta i bit flags.

Nie istnieje `return_500ms`. Projekcja nie deklaruje complete event trajectory.
Reference sample jest ostatnią próbką o timestampie nie większym od targetu;
zbyt odległy reference daje `None`. Reversed slot/timestamp ordering i invalid
price dają `Invalid`. Stary newest sample daje `Stale`. Wiele kanonicznych
update'ów pomiędzy tickami ustawia jawny `COLLAPSED_CANONICAL_UPDATES`; pole
historycznie nazwane `tx_count` jest opisane jako licznik canonical state
updates, a nie authoritative trade count.

## 3. Snapshot bundle i izolacja TimeStop V2

Engine materializuje jeden `PostBuySnapshotBundle` pod jednym read lockiem:

- istniejący `PostBuyDecisionSnapshot` pozostaje bazą V1;
- V2 extras zawierają wyłącznie trajectory, immutable vitality projection,
  route status, executable anchor, entry-value contract i run ID;
- pola V1 nie są kopiowane do konkurencyjnego snapshotu.

`TimeStopV2State::evaluate()` nadal należy wyłącznie do istniejącej ścieżki
TimeStop. Dodano read-only `project()`, z którego powstaje
`TimeStopV2ProjectionV1`, a następnie `VitalityFeaturesV1`. HET-PM:

- nie wywołuje `evaluate()`;
- nie modyfikuje `pos.time_stop_v2`;
- nie emituje dodatkowych istniejących TimeStop rows;
- mapuje Alive/Weak/Heartbeat/Stale wyłącznie z immutable projection.

Test parity potwierdza identyczne state transitions i istniejące rows przy HET
enabled/disabled.

## 4. Entry value i route contract

Entry-value precedence jest jawna:

1. persisted `entry_size_lamports` — authoritative dla shadow comparison;
2. `entry_price × entry_token_amount` — wyłącznie diagnostic fallback;
3. typed unavailable.

Route jest osobnym kontraktem, niezależnym od trajectory quality:

- aktywna, nieukończona PumpCurve: `PumpCurveSupported`;
- ukończona curve wymagająca PumpSwap: typed unsupported;
- bootstrap/pending lub niepewna route: typed unknown.

PR A nie dodaje PumpSwap quote/builder i nie porównuje anchora między venue.

## 5. Pure ExitPolicyV2

Nowy `exit_policy_v2.rs` nie posiada mutable runtime state, locków, async, RPC,
writerów ani executora. Zwraca wyłącznie typed prequote/final decision i
diagnostyczne suppressed-gate bits.

Hierarchia jest deterministyczna:

```text
Pending
  > Integrity/data/route blockers
  > existing CrashGuard candidate
  > existing V1 hard loss candidate
  > executable trailing candidate
  > recovery-aware vitality decay
  > existing absolute max-hold
  > Hold
```

Typed blocker nie jest zamieniany na `Hold`. Trailing wymaga dodatniego mark
arm, mark drawdown, porównywalnego executable anchora i current full-position
quote. Vitality wymaga wieku, odpowiedniej liczby non-alive windows, świeżego
trajectory/vitality evidence, braku nowego peak i braku 5 s recovery. W PR A
każde `ExitAll` jest wyłącznie counterfactual evidence.

## 6. Executable peak anchor

`ExecutablePeakAnchorV1` jest prywatnym observer state, nie policy/action
ownerem. Request powstaje tylko wtedy, gdy newest canonical sample jest nowym
mark peak oraz:

- anchor nie istnieje; albo
- nowy peak przekracza minimalny step; albo
- nastąpił force interval, ale nadal na nowym peak event.

Anchor:

- nigdy nie przesuwa się w dół;
- nie wygasa wyłącznie przez wiek;
- zapisuje position/epoch/quantity/route/quote-model/config-hash/revision i
  source sample;
- posiada własny monotoniczny `anchor_seq`;
- nie zwiększa ekonomicznego `state_revision`;
- jest stosowany dopiero po V1 ticku i tylko przy nadal aktualnym guardzie;
- nie jest stosowany po terminalizacji V1.

Porównanie current quote z anchorem failuje typed blockerem dla mismatchu
position, epoch, quantity, route, quote model, config hash lub revision.

## 7. Quote planning i same-tick contract

Lokalny `HetPmV2QuotePlan` ma statyczny limit dwóch komórek. Pełny klucz
zawiera:

```text
position_id, position_epoch, state_revision, remaining_quantity_raw,
route_id, quote_model_id, sample_slot, sample_timestamp_ms
```

Właściwości:

- zwykły V1+V2 Hold nie tworzy quote cell;
- anchor-only albo trailing tworzy najwyżej jeden current cell;
- identyczny V1/V2/anchor key jest rozwiązywany raz;
- różny raw canonical i runtime-projected key ma osobną komórkę;
- nie ma cache między tickami;
- full remaining quantity zawsze jest częścią key;
- V1 source/provenance nie jest podmieniane przez V2.

Komórka przechowuje zarówno `Ok(ShadowExitTruth)`, jak i typed
`Err(ExecutableQuoteFailure)`. Dzięki temu również nieudana próba jest
współdzielona z V1 dla identycznego key; V1 nie wykonuje drugiego resolver call
w tym samym ticku. Stale/revision mismatch nie jest współdzielony.

V1 i V2 są obliczane z pre-mutation bundle. Następnie V1 wykonuje swój
dotychczasowy guarded apply/terminal flow. Dopiero po nim observer próbuje
anchor apply i zapisuje comparison. Pending terminal retry nie tworzy nowej V2
oceny ani quote.

## 8. Sidecar, terminal isolation i fail-open durability

Jedyny nowy runtime artefakt to:

```text
het_pm_v2_observations_v1.jsonl
```

Jest to bounded, wersjonowany sidecar poza canonical terminal commit. Rekord
zawiera policy/schema/config identity, pre-mutation position identity/revision,
V1/V2 decisions, gate/suppression, trajectory/vitality/route, entry contract,
anchor before/request/applied, quote keys/statuses i observer-only isolation
markers.

Przed appendem rekord:

- sprawdza policy/schema/sampling/measurement grade;
- wymaga shadow lane i `consumed_by_policy=false`;
- odrzuca każdy marker ekonomicznej/proposal/TimeStop/route/terminal mutacji;
- wymaga maksymalnie dwóch quote cells i spójnych cardinalities;
- odrzuca non-finite metrics;
- ma limit 64 KiB;
- jest pre-serializowany.

Serialization, validation, oversized payload i writer failure powodują
pominięcie wyłącznie sidecara. Nie są elementem `TerminalCommitReceipt`, nie
mogą zmienić `canonical_committed()`, cleanup, capacity release ani utworzyć
`PendingTerminalCommit`.

Probe monitor ma sidecar jawnie wyłączony, więc istnieje jeden writer dla
primary monitor.

## 9. Konfiguracja i startup

Dodano serde-default `[post_buy_guardian.het_pm_v2]`. Brak sekcji oznacza:

```text
enabled = false
```

Unknown mode nie deserializuje się. `authoritative_shadow` jest odrzucane przy
startupie PR A we wszystkich execution modes. HET config validation wymaga
poprawnych okien, kolejności, sample age, bps, anchor step/refresh i vitality
windows/age. HET hash obejmuje wyłącznie jawne pola HET, nie V1 config.

Aktywny `ghost_brain_config.toml` ustawia dokładne safe initial shadow
hypotheses z planu oraz `mode = "observe_only"`. Rust default CrashGuarda
pozostaje `Disabled`; aktywny main profile nadal jawnie ustawia
`crash_guard_mode = "observe_only"`. Włączenie/wyłączenie HET nie zmienia
effective CrashGuard mode ani V1 hash/thresholdów.

Startup log emituje policy ID/version/schema/hash, mode, sampling/grade,
trajectory windows, wszystkie trailing/anchor/vitality hypotheses, effective
CrashGuard mode i source oraz jawne authority flags:

```text
V1 shadow authority = true
V2 shadow authority = false
live authority = false
```

## 10. Offline analysis

Dodano deterministyczny, stdlib-only `scripts/het_pm_v2_analysis.py` z testem
kontraktu. Narzędzie:

- czyta wyłącznie sidecar schema v1 i failuje na brak/unsupported schema;
- hashuje wszystkie inputy;
- używa `(position_id, position_epoch)` jako denominatora pozycji;
- raportuje lifecycle isolation, trajectory/anchor/route coverage, quote
  budget/blockers, gate counts oraz gross executable cost scenarios;
- rozróżnia quote należący do predecyzji V1 od zwykłego V1+V2 `Hold`, aby
  współdzielony same-tick quote nie został błędnie przypisany polityce V2;
- nie nazywa gross return wartością net;
- nie ustawia promotion pass;
- jawnie oznacza between-tick cache reuse i pełne counterfactual outcome
  attribution jako nieweryfikowalne z samego sidecara.

Pełny promotion artifact, zamrożone criteria i lifecycle/replay future-outcome
join pozostają wymaganiem przed PR B. PR A nie może być użyty jako ręczny
promotion gate.

## 11. Zakres celowo wyłączony

PR A nie:

- zmienia BUY/REJECT/TIMEOUT ani Gatekeepera;
- zmienia quantity, proposal, action ID, close reason, terminal truth, capacity
  albo economic revision;
- aktywuje V2 shadow authority lub live execution;
- dodaje partial exits, runnera, ladders, AEM/Revolver authority;
- dodaje cost reserve ani authoritative net PnL;
- dodaje PumpSwap builder/quote;
- przywraca legacy HyperPrediction/Chaos/old score path;
- tworzy drugiego ownera/store/buffera/writera/commit pointu.

## 12. Walidacja lokalna

Wykonano względem base SHA `18d94b0cc5a226496a5ac2bc616e7488a7f78d5d`:

| Kontrola | Wynik |
| --- | --- |
| `cargo check -p ghost-brain --lib` | PASS; istniejące warnings workspace pozostają. |
| `cargo test -p ghost-brain guardian::post_buy --lib` | PASS — 209 testów; trajectory, pure policy, anchor, quote plan, TimeStop parity, same-tick i terminal fault isolation. |
| focused `guardian::post_buy::trajectory_v1` | PASS — projekcja po końcowym usunięciu produkcyjnego `expect()`. |
| `cargo test -p ghost-brain --test ghost_brain_config_load_test` | PASS — 7 testów active profile/backward compatibility. |
| `cargo test -p ghost-brain events::validator --lib` | PASS — 12 testów. |
| `cargo test -p ghost-launcher --test post_buy_runtime_integration` | PASS — 4 testy. |
| `cargo test -p trigger entry_price_extractor` | PASS — 10 testów. |
| `cargo test -p ghost-launcher --test gatekeeper_v25_regression` | PASS — 42 testy. |
| `cargo test -p ghost-launcher --test gatekeeper_v3_tests` | PASS — 9 testów. |
| `python3 -m unittest scripts/test_het_pm_v2_analysis.py` | PASS — 3 testy determinism/schema fail-closed/quote ownership. |
| `python3 -m py_compile scripts/het_pm_v2_analysis.py scripts/test_het_pm_v2_analysis.py` | PASS. |
| diff-scoped `cargo clippy` dla zmienionych plików post-buy i launchera | PASS; pełny crate-level Clippy zatrzymuje istniejący `never_loop` w `pipeline/execution.rs:1569`, poza diffem PR A. |
| forbidden-scope allowlist audit | PASS — brak zmian w prebuy, Gatekeeper policy, builderach/senderach i live execution; launcher diff obejmuje wyłącznie walidację/status/wiring post-buy oraz test. |
| `cargo fmt --all -- --check` | PASS. |
| `git diff --check` | PASS. |

### 12.1. Znany niezwiązany baseline failure

Pełne:

```text
cargo test -p ghost-launcher components::post_buy_runtime --lib
```

zakończyło się `66 passed / 1 failed`. Failuje istniejący test:

```text
shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff
```

z błędem braku pliku density JSONL (`No such file or directory`). Failure
odtworzono również samodzielnie. Dotyczy istniejącego Shadow V2 density smoke,
nie HET config/startup/sidecar/quote path; diff PR A nie zmienia emitera density.
Zgodnie z zasadą wąskiego zakresu PR A nie naprawia niezwiązanego subsystemu.
Nie deklarujemy więc całego workspace CI jako zielonego, mimo że wszystkie
zmienione kontrakty PR A i pozostałe wymagane pakiety przechodzą.

## 13. Rollback

Rollback to pełny revert PR A. Nie ma migracji pozycji ani canonical schema.
Stary TOML bez sekcji HET automatycznie ładuje V2 jako disabled, ale częściowy
revert samego sidecara, snapshotu lub anchor state pozostawiłby niepełny
kontrakt evidence, dlatego nie jest zalecany.

Po rollbacku V1 pozostaje dokładnie tym samym jedynym shadow authority i nie
ma zmiany live behavior.

## 14. Mapa implementacji

- `ghost-brain/src/guardian/post_buy/trajectory_v1.rs` — pure sampled trajectory;
- `ghost-brain/src/guardian/post_buy/exit_policy_v2.rs` — config validation,
  bundle extras, immutable projection, route/entry/anchor/policy/record contracts;
- `ghost-brain/src/guardian/post_buy/config.rs` — serde-default HET config;
- `ghost-brain/src/guardian/post_buy/engine.rs` — bundle, quote plan, same-tick
  orchestration, observer anchor i fail-open sidecar;
- `ghost-brain/src/guardian/post_buy/mod.rs` — moduły i publiczny startup config/status surface;
- `ghost-launcher/src/components/post_buy_runtime.rs` — startup validation/status
  i single-writer wiring;
- `ghost-brain/ghost_brain_config.toml` — aktywny observe-only profile;
- `ghost-brain/tests/ghost_brain_config_load_test.rs` — active config contract;
- `scripts/het_pm_v2_analysis.py` — deterministyczna analiza PR A;
- `scripts/test_het_pm_v2_analysis.py` — testy offline contractu.
