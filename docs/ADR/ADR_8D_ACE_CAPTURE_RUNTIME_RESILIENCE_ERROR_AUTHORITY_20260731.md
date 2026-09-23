# ADR-8D: ACE Day-1 capture — odporność runtime’u i jawna władza błędów

**Data:** 2026-07-31
**Status:** IMPLEMENTED LOCALLY / FOCUSED VALIDATION PENDING / RESILIENCE SOAK PENDING / DAY 1 NO-GO
**Zakres:** aktywny, shadow-only ACE reality capture z `configs/rollout/ace-core-one-day-probe-r1.toml`; bez zmiany strategii, Gatekeepera, Triggera, live execution ani PR2.

## D0. Problem i granica decyzji

Nieważny Day 1 zakończył launcher po pojedynczym błędzie odświeżenia RPC:

```text
RUG_SCALP_RUNTIME_FEE_AUTHORITY_INVALIDATED
→ RUG_SCALP_RUNTIME_FEE_AUTHORITY_CHANGE_REQUESTED_CONTROLLED_SHUTDOWN
→ /metrics disappears as a consequence of shutdown
```

W rolloutcie ACE `trigger.enabled=false`, `execution_mode=shadow`, `rug_scalp_v2.enabled=false` i `rug_reality_capture.enabled=true`. Typed fee authority jest zamrażane w immutable capture manifeście podczas startu i konsumowane offline przez ACE probe. Runtime fee watch dla RUG V2 nie jest canonical ingestem ani konsumentem ACE, więc nie może posiadać shutdown authority.

Decyzja oddziela brak możliwości udowodnienia kompletności jednego segmentu od awarii, która czyni wewnętrzny registry albo podstawowy canonical ingest niespójnym. W pierwszym przypadku launcher zachowuje późniejszy tape, a finalizer odrzuca run. W drugim pozostaje globalny fail-close.

## D1. Canonical taxonomy error authority

| Klasa | Dopuszczony skutek | Zakaz |
|---|---|---|
| `CandidateLocal` | BLOCKED/reclaim tej mutacji lub kandydata, bez permitu | global admission close albo launcher shutdown |
| `OptionalLaneDegraded` | typed counter, log, zatrzymanie/degradacja własnego lane | wpływ na canonical ingest |
| `TransientExternalDependency` | typed counter, surowa klasa błędu, bounded retry/backoff i last-known-good | timeout/429/5xx/reset jako invalidacja authority |
| `RecoverableTransportGap` | typed evidence i późniejsza jawna ocena completeness | cichy drop albo process kill |
| `CaptureSegmentInvalid` | `ace_capture_segment_invalid_total`; finalizer/probe odrzuca run | zamknięcie admission lub fake `VALID_CAPTURE` |
| `GlobalRuntimeFatal` | istniejący global fail-close / core runtime termination | użycie dla błędu zewnętrznego lub candidate-local |

`CaptureFailureClassV1` jest wspólną klasyfikacją nowych granic Seer/Oracle/watchdog/RUG. Wyłącznie `GlobalRuntimeFatal` zwraca `closes_candidate_admission() == true`.

## D2. Pełny audit runtime-killing callsite’ów w aktywnym ACE stacku

Tabela obejmuje wszystkie znalezione callsite’y posiadające lub potencjalnie posiadające władzę: `close_candidate_admission*`, launcherowy shutdown, `process::exit`, periodic RPC watch, IPC gap/drop i monitoring. Ścieżki Trigger/live/P37/Shadow-V2 są ujęte także wtedy, gdy rollout je wyłącza.

| Callsite / aktywacja | Trigger poprzednio | Poprzedni skutek | Klasa teraz | Nowy skutek w ACE |
|---|---|---|---|---|
| `main.rs::materialize_rug_fee_authority_*` przed manifestem | timeout/429/5xx/DNS pobrania dwóch wymaganych kont | pojedynczy błąd kończył start | `TransientExternalDependency` per próba | 5 bounded prób z exponential backoff+jitter; przed sukcesem nie istnieje aktywny capture |
| `main.rs::RugScalpRuntimeFeeAuthorityWatch` | runtime RPC refresh lub authority change | pierwszy `Err` invalidował authority i wysyłał shutdown | `OptionalLaneDegraded` / `TransientExternalDependency` | **nie startuje** gdy `rug_scalp_v2=false`; w świadomym V2 retry/advisory-only, bez shutdown/admission |
| `main.rs` gRPC subscribe-proof timer | brak proof po 5 s | `process::exit` | `TransientExternalDependency` | ACE loguje delayed advisory; stream może się odzyskać |
| `components/watchdog.rs` | utrwalony gRPC/event-writer stall po istniejącym progu 5 min | `process::exit(2..4)` | `CaptureSegmentInvalid` dla canonical tape, `OptionalLaneDegraded` dla decision writer | ACE używa `AdvisoryCapture`: brak exit/admission close; trwała luka trafia wyłącznie do finalizera |
| `main.rs` Oracle task ending early | Oracle core ended/error/panic | fatal exit | `GlobalRuntimeFatal` | zachowane: canonical consumer nie może umrzeć po cichu |
| `main.rs` Seer task ending early | Seer core ended/error/panic | fatal exit | `GlobalRuntimeFatal` | zachowane: canonical producer nie może umrzeć po cichu |
| `main.rs` Shadow-V2 artifact budget guard | optional Shadow-V2 budget | global shutdown sender | inactive for ACE | guard nie startuje w ACE i nie ma authority nad capture |
| `main.rs` metrics server | registration/bind error | log-only | `OptionalLaneDegraded` | bez shutdown authority; liveness jest osobna od scrape |
| `scripts/ace_core_capture_supervisor.py` | timeout/429/5xx/reset `/metrics` | operator mógł mylnie uznać scrape za root cause | `TransientExternalDependency` | retry z bounded backoff, `Popen.poll`, last-known-good i lifecycle JSON; scraper nigdy nie wysyła SIGINT z powodu błędu |
| supervisor final scrape | endpoint znika podczas controlled shutdown | brak końcowego HTTP maskował pierwotny reason | `OptionalLaneDegraded` obserwacji | zapis lifecycle i prawdziwego exit reason; valid receipt nadal wymaga obu snapshotów |
| `components/seer.rs::apply_candidate_integrity_failure_policy` | `record_signal`, stage, inventory seal, lease error | kilka ogólnych `Err(_)` zamykało admission | mapowane przez `CandidateIntegrityErrorV1` | local error blokuje tylko mutację; segment capacity invaliduje finalizerem; tylko registry fatal zamyka admission |
| `CandidateAliasConflict` w Seer/Oracle acknowledgement | aliasy jednej mutacji się nie zgadzają | wtórny generic close | `CandidateLocal` | receipt fail/reclaim, brak permitu/Ready, admission open |
| terminal cleanup / observation lease | terminal barrier, lokalny receipt, candidate zakończony | część errorów eskalowała globalnie | `CandidateLocal` poza internal contradiction | barrier blokuje tylko candidate; ledger/receipt obligation pozostaje terminalna |
| `PumpObservationLedger::observe/finalize` mutex poison | rzeczywisty poisoned ledger lock | global close | `GlobalRuntimeFatal` | zachowane: canonical ledger state jest niewiarygodny |
| primary `LocalProcessingGap` / `EvidenceCapacityExceeded` | niepełność primary interval | global admission close | `CaptureSegmentInvalid` | PR1 gap + ACE segment counter; launcher dalej zapisuje późniejszy tape, finalizer odrzuca dzień |
| local coverage control overflow/closed | brak pełnego control prefixu | IPC receiver mógł wykonać `break` | `CaptureSegmentInvalid` | marker i wyłączenie tylko control branch; receiver dalej zbiera tape |
| optional BCV2 hydration / `RpcMissing` | queue full, worker closed, enrichment RPC unavailable | ryzyko pobocznego runtime authority | `CandidateLocal` / `RecoverableTransportGap` | typed `RpcMissing` przez istniejące bounded IPC; brak task-per-failure i brak admission close |
| `off-chain/.../ipc.rs::EventDropped` | optional/coalescible evidence | typed drop | `OptionalLaneDegraded` | bez global authority; nie udaje primary trade |
| `off-chain/.../ipc.rs::LocalProcessingGap` | brak miejsca dla required delivery | primary gap może nastąpić | `CaptureSegmentInvalid` | typed gap/control evidence; finalizer invaliduje zamiast launcher kill |
| `oracle_runtime.rs` Event Bus `RecvError::Lagged` | receiver lost history | generic close dla reclaim errors | `CaptureSegmentInvalid`, chyba że reclaim ujawnia corruption | pending receipts są rozliczane; later tape trwa, finalizer odrzuca incomplete segment |
| `oracle_runtime.rs::complete_canonical_apply` missing receipt | downstream canonical apply bez fence | global close | `GlobalRuntimeFatal` | zachowane: łamie PR1E permit/apply invariant |
| `oracle_runtime.rs::send_pool_observation_result` closed result channel | terminal owner nie może raportować do Oracle | global close | `GlobalRuntimeFatal` | zachowane: core lifecycle ownership vanished |
| `candidate_integrity.rs::lock_state`, force-close, `mark_unavailable` | true `PoisonError`, unavailable registry, impossible fence/record contradiction | global close | `GlobalRuntimeFatal` | zachowane bez `PoisonError::into_inner` i bez resetu registry |
| registry/fence capacity | bounded candidate/receipt capacity exhausted | global close / unavailable | `CaptureSegmentInvalid` | brak silent dropu; segment counter wymusza offline INVALID, admission open |
| `EventEmitter` durable writer | write/lock error w ACE JSONL | log and continue | `CaptureSegmentInvalid` at finalization | health finalizer znajduje marker i nie tworzy receipt |
| health receipt / ACE probe | snapshot missing, PR1 counter, segment invalid, dirty JSONL | operatorowa niejednoznaczność | offline `INVALID_CAPTURE` | fail-closed receipt/probe, bez fake valid |
| Trigger/live sender/PostBuy/PM/P37/RUG V2 reducer | ich własne RPC/error paths | aktywne w innych rolloutach | inactive in ACE | `trigger=false`, `rug_scalp_v2=false`, `p37=false`; nie startują i nie sterują ACE |

### D2.1. Startup exits, które nie są runtime kill switchami

`main.rs` zachowuje `process::exit(1)` dla nieprawidłowego configu, niewritable canonical output/WAL, niepoprawnego Gatekeeper runtime contract lub wymaganego live sendera. Są wykonywane przed startem komponentów. W ACE Trigger jest wyłączony, więc live sender/keypair path nie jest osiągalny. Nie są dowodem, że pojedynczy transient błąd może zakończyć już aktywny 24h capture.

## D3. Zachowane global-fatal invariants

Tylko poniższe klasy zachowują globalną władzę w aktywnym ACE runtime:

1. rzeczywisty `Mutex::lock() -> PoisonError` lub `available=false` registry, którego stan nie jest już wiarygodny;
2. niemożliwa wewnętrzna sprzeczność identity/proof/fence, np. zniknięty terminal record albo canonical apply bez receipt;
3. nieodwracalna awaria core `PumpObservationLedger` mutexu;
4. śmierć/awaria core Seer lub OracleRuntime przed operatorowym shutdownem;
5. brak core Oracle `PoolObservationResult` delivery;
6. trwały EventWriter/canonical tape error po bounded recovery: launcher zachowuje later tape, ale finalizer obligatoryjnie unieważnia capture;
7. jawny operatorowy SIGINT/SIGTERM.

Timeout RPC, 429, 5xx, DNS, connection reset, pojedynczy missing optional evidence, candidate alias conflict i pojedynczy recovery gap nie należą do tej listy.

## D4. Supervisor i health evidence

`scripts/ace_core_capture_supervisor.py` jest observerem, nie kontrolerem runtime:

```text
launcher starts in a new process session
→ wait for immutable manifest
→ bounded /metrics retries with last-known-good state
→ write manifest-bound start snapshot on first complete scrape
→ final pre-SIGINT snapshot if available
→ SIGINT only because requested duration elapsed
→ persist lifecycle/exit reason even when endpoint is gone
```

Supervisor nie tworzy health receipt. Offline finalizer nadal wymaga obu manifest-bound snapshotów, czystego EventWriter, zero krytycznych counters i controlled shutdown. Przy jednym nieudanym końcowym scrape może zapisać end snapshot z ostatniego poprawnego odczytu wyłącznie w krótkim, skonfigurowanym oknie; snapshot zachowuje jego rzeczywisty timestamp i `source=last_known_good`, więc nie fałszuje świeżości. Dłuższa utrata metrics nadal daje invalid capture, lecz lifecycle artefakt zapisuje realną przyczynę zamiast nazywać zniknięcie portu root cause.

## D5. Niezmienione granice

Nie zmieniono ACE cech/cutoffu/kalibracji/probe, Gatekeepera, MFS, Brain policy, capacity, Triggera, live execution, Position Managera, P37, PR2, IPC capacity/backpressure ani fail-closed health/probe semantics dla niekompletnego tape.

## D6. Weryfikacja i bramka przed nowym Day 1

```bash
cargo fmt --all --check
cargo test -p ghost-launcher --lib capture_resilience -- --nocapture
cargo test -p ghost-launcher --lib candidate_integrity -- --nocapture
cargo test -p ghost-launcher --lib alias_conflict -- --nocapture
cargo test -p ghost-launcher --lib primary_local_coverage_gap -- --nocapture
cargo test -p ghost-launcher --lib rug_scalp_v2 -- --nocapture
cargo test -p ghost-launcher --bin ghost-launcher -- --nocapture
python3 -m unittest scripts/test_ace_core_one_day_capture_health.py scripts/test_ace_core_capture_supervisor.py scripts/test_ace_capture_metrics_fault_proxy.py
cargo build --release -p ghost-launcher --bin ghost-launcher --bin ace_core_one_day_probe
```

Następnie niezależny, 30-minutowy observe-only fault-injection soak musi wykazać timeout, 429, 500, connection reset i okresową niedostępność metrics bez globalnego shutdownu. Lokalny fault proxy nie ma kanału shutdown i tylko pośredniczy w HTTP. Po SIGINT wymagane są oba markery drenu, `sent == received`, zero backlogów, zero trzech PR1E counters, `ace_capture_segment_invalid_total=0`, `finalize=0` i `verify-probe=0`.

Do czasu niezależnego review i pozytywnego soaku Day 1 pozostaje **NO-GO**.
