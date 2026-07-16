# ADR-8D: Position Manager Lite V1 PR2 — absolute max-hold i CrashGuard V1

Status: `IMPLEMENTED / LOCAL VALIDATION PASSED / CI NOT YET RUN`

Typ: ADR-8D / Position Manager Lite V1 PR2 / aktywny shadow post-buy / evidence i exit-policy safety

Data: 2026-07-15

Repo: `smahacfel/Gho`

Branch: `agent/position-manager-lite-pr2-20260715`

Base SHA: `5853f4fda2430b0101cab4dd38ee2e7cbcbee90f`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_LITE_V1.md`, PR 2.

Poziom ryzyka: `MEDIUM` — zmiana rozszerza aktywną politykę shadow o
absolute max-hold i dostarcza obserwacyjny CrashGuard. Nie aktywuje live
execution, partial exits, AEM authority, Guardian authority ani Revolvera.

## 1. Problem i decyzja

Po PR1 aktywna ścieżka shadow miała jeden pure `ExitPolicyV1`, guarded apply,
lazy full-position quote i durable terminal commit. Brakowało jednak dwóch
elementów biznesowych potrzebnych do wiarygodnego post-buy burn-in:

1. niezależnego od heartbeatów absolute max-hold, który ogranicza occupation
   jedynego slotu przez pozycję z pozorną aktywnością;
2. zwartego, jakościowo opisanego sygnału nagłego crashu, który można najpierw
   mierzyć bez zmiany lifecycle.

Decyzja PR2:

```text
private MonitoredPosition state
  -> immutable PostBuyDecisionSnapshot + compact CrashVectorV1
  -> pure ExitPolicyV1
  -> tylko po realnym kandydacie: jeden lazy position-sized quote
  -> guarded proposal/apply lub observation-only lifecycle record
```

`CrashGuard` w produkcyjnym profilu pozostaje **wyłącznie
`observe_only`**. Nie tworzy pending SELL, nie zmienia quantity, close reason,
terminal truth ani capacity. `authoritative_shadow` istnieje jako jawnie
zabezpieczony przyszły tryb, ale nie jest promowany przez ten PR ani TOML.

## 2. Zmiana konfiguracji i kompatybilność

Dodano serde-default `ExitPolicyV1Config` pod
`[post_buy_guardian.exit_policy_v1]`.

Brak tej sekcji w historycznym TOML zachowuje dokładnie kontrakt PR1:

- `absolute_max_hold_enabled = false`;
- `crash_guard_mode = "disabled"`;
- istniejące +50%, -50%, 30 s inactivity i 5 s recovery nie ulegają zmianie.

Aktywny `ghost_brain_config.toml` ustawia jawnie:

```toml
quote_recovery_ms = 5000
absolute_max_hold_enabled = true
absolute_max_hold_ms = 120000
crash_guard_mode = "observe_only"
crash_window_ms = 1500
crash_min_short_window_drop_pct = 25.0
crash_min_peak_drawdown_pct = 30.0
crash_min_distinct_slots = 2
crash_max_sample_age_ms = 1500
crash_max_executable_return_pct = -20.0
```

`PostBuyRuntime` odrzuca `authoritative_shadow`, jeżeli profil nie jest
kompletnym shadow profilem: `execution_mode=shadow`,
`entry_mode=shadow_only`, brak live sendera, wired canonical shadow monitor
oraz obecne lifecycle i canonical evidence output. Nie istnieje automatyczna
promocja z `observe_only`.

## 3. Absolute max-hold

Nowy candidate `AbsoluteMaxHold`:

- korzysta wyłącznie z `now_ms - entry_unix_ms`;
- zaczyna działać dokładnie przy `120_000 ms`;
- nie może zostać zresetowany przez market-activity heartbeat;
- pozostaje za stop-loss, take-profit i inactivity w kolejności baseline;
- ma identyczny lazy quote, pending recovery, guarded apply i terminal contract
  jak dotychczasowy full exit;
- nie wprowadza partial exit.

Przy jednoczesnym inactivity i max-hold zwycięża `Inactivity`. Lifecycle
zapisuje reason, policy/config hash, absolute age, inactivity age,
capacity-occupancy age i `would_hold_under_legacy_inactivity_policy`.

## 4. CrashGuard V1 i provenance

CrashGuard materializuje kompaktowy `CrashVectorV1`, bez kopiowania całej
historii i bez dostępu policy do mutable state. Candidate wymaga łącznie:

- co najmniej dwóch poprawnych kolejnych próbek z różnych slotów;
- poprawnej kolejności slot/timestamp;
- monotonicznego spadku;
- newest raw canonical sample nie starszego niż 1500 ms;
- spadku oldest-to-newest w oknie 1500 ms co najmniej 25%;
- drawdownu od kanonicznego peak co najmniej 30%.

Peak aktualizuje się wyłącznie po przyjęciu poprawnego kanonicznego snapshotu,
nie przez AEM ani Guardian signals.

Najważniejszy kontrakt provenance: CrashGuard czyta **raw canonical timeline**.
Runtime może utworzyć "current" projection dla starego stanu przy obsłudze
legacy time-stop, ale jego observed-at timestamp nie może odświeżyć stale
crash evidence. Odpowiedni lifecycle record otrzymuje `truth_status=stale`,
gdy raw sample przekracza crash freshness limit.

Po tanim candidate wykonywany jest najwyżej jeden local, niecache'owany między
tickami, position-sized executable quote. `CrashConfirmed` wymaga pełnej
pozostałej quantity, executable output, quote z tej samej/nowszej świeżej
revision oraz executable gross return nie wyższego niż -20% względem entry.

Każdy transition `NotTriggered`, `Candidate`, `Confirmed`,
`RejectedByQuote` albo `BlockedByData` ma observation lifecycle record z:

- `authoritative_decision`;
- `crash_guard_candidate_decision`;
- raw slot/timestamp provenance i metrykami vectora;
- `crash_guard_consumed_by_policy=false` w aktywnym profilu.

Te obserwacje nie są canonical terminal truth i nie zmieniają outcome pozycji.

## 5. Jedno quote i przyszły tryb authoritative

Jeżeli baseline i CrashGuard wymagają quote w tym samym ticku, engine używa
jednej lokalnej quote resolution. Quote nie jest przechowywany między tickami.

Dodatkowo future-safe `authoritative_shadow` nie może usunąć ważnego
baselineowego exit: gdy CrashGuard dostanie resolved quote, ale odpadnie na
własnym progu, istniejący guarded action jest retargetowany na baseline
SL/TP/time candidate. Zachowuje ten sam `action_id`, quantity, recovery window
i pojedynczy quote; nie powstaje drugi sell ani druga ścieżka execution.

## 6. Zakres celowo wyłączony

PR2 nie:

- aktywuje live sendera, live dispatchu ani zmienia +58%/-46% dormant lane;
- nie promuje CrashGuard do authority w aktywnym configu;
- nie dodaje partial exits, trailing stopów, WaitReclaim ani dynamicznych
  AEM regimes;
- nie przywraca Revolver bullets ani `ShadowPositionBook` jako state ownera;
- nie używa LIGMA/WHF/TCF/PANIC composite score jako hard triggera;
- nie zmienia Gatekeepera, prebuy Decision Plane ani Type-5.

Jedyna korekta poza post-buy jest naprawą istniejącego testu/komentarza
konfiguracyjnego: aktywny `gatekeeper_v2.min_market_cap_sol` od dawna wynosi
`115.0`, podczas gdy test oczekiwał `5.0`, a nagłówek TOML opisywał `48`.
PR2 wyrównuje test i komentarz do faktycznej, **niezmienionej** wartości `115.0`;
nie zmienia żadnego progu Gatekeepera.

## 7. Inwarianty

- policy pozostaje pure: brak locków, RPC, `Instant`, executora i mutable
  runtime state;
- pola pozycji i snapshotu są prywatne; mutacja idzie wyłącznie przez guarded
  begin/apply/terminal interface;
- `Hold` oraz zwykłe `UnknownEvidence` nie uruchamiają quote;
- failed/missing quote po proposal nadal używa bounded recovery i nie tworzy
  resolved close;
- obserwacyjny CrashGuard nie wpływa na lifecycle ani capacity;
- shadow `SimulationBlocked` nie jest live `Unknown`, a live unknown nadal nie
  zwalnia capacity;
- canonical `ShadowTerminalTruthV2` pozostaje commit pointem PR1;
- bounded timeline pozostaje bounded.

## 8. Rollback i następny krok

Rollbackiem jest pełny revert PR2. Stary profil bez nowych pól już przywraca
wyłączony max-hold i CrashGuard bez zmiany kodu konfiguracji. Nie należy
częściowo revertować tylko logów albo tylko policy, ponieważ rozdzieliłoby to
reason/provenance od rzeczywistego zachowania.

Po merge wymagany jest shadow burn-in z outcome contractem PR1/PR2. Dopiero
po zebraniu stabilnych executable outcomes można kalibrować max-hold i
CrashGuard lub rozpocząć Type-5 Lite jako observe-only. Promocja CrashGuarda,
AEM albo partial exits pozostaje osobną decyzją opartą o dane.

## 9. Walidacja lokalna

Wykonano na branchu `agent/position-manager-lite-pr2-20260715`, względem
`5853f4fda2430b0101cab4dd38ee2e7cbcbee90f`:

| Kontrola | Wynik |
| --- | --- |
| `cargo test -p ghost-brain guardian::post_buy::engine::tests --lib --quiet -- --test-threads=1` | PASS — 51 testów, w tym exact-boundary max-hold, CrashGuard negative predicates, stale raw evidence, retry/retarget i observe-only non-interference. |
| `cargo test -p ghost-brain guardian::post_buy::exit_policy_v1::tests --lib --quiet -- --test-threads=1` | PASS — 15 testów pure policy i source guard. |
| `cargo test -p ghost-brain guardian::post_buy::config::tests --lib --quiet -- --test-threads=1` | PASS — 10 testów serde-default i exact PR2 config. |
| `cargo test -p ghost-brain events::validator::tests --lib --quiet -- --test-threads=1` | PASS — 12 testów lifecycle position/epoch. |
| `cargo test -p ghost-brain --test ghost_brain_config_load_test --quiet` | PASS — 7 testów; aktywny TOML oraz serde-default historycznego profilu. |
| `cargo test -p ghost-launcher components::post_buy_runtime --lib --quiet -- --test-threads=1 --skip shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff` | PASS — 65 testów runtime/config guardów. Wyłączony marker jest istniejącym baseline’em PR1: helper smoke nie tworzy wymaganej density projection i nie dotyczy diffu PR2. |
| `cargo test -p ghost-launcher --test post_buy_runtime_integration --quiet -- --test-threads=1` | PASS — 4 testy integracyjne. |
| `cargo test -p trigger entry_price_extractor::tests --lib --quiet` | PASS — 10 testów. |
| `cargo test -p ghost-launcher --test gatekeeper_v25_regression --quiet` | PASS — 42 testy; PR2 nie zmienia prebuy. |
| `cargo test -p ghost-launcher --test gatekeeper_v3_tests --quiet` | PASS — 9 testów; PR2 nie zmienia prebuy. |
| `cargo test -p ghost-launcher --test metric_contracts_pr2c_replay --quiet` | PASS — 10 testów replay metric contracts. |
| `cargo test -p ghost-brain --test oracle_decision_logger_integration --quiet` | PASS — 4 testy logger/replay. |
| `cargo test -p ghost-brain --lib replay_payload --quiet` | PASS — 5 testów replay payload. |
| `python3 -m unittest scripts.test_guard_restore_shadow_lifecycle -v` oraz `python3 scripts/guard_restore_shadow_lifecycle.py --skip-runtime --output-dir /tmp/position_manager_lite_pr2_restore_guard --json` | PASS — 10 testów guardu oraz `RESTORE_PATH_STATIC_GUARD_PASS`; runtime smoke celowo skipped przez flagę. |
| `cargo fmt --all -- --check` i `git diff --check` | PASS. |

### Scoped Clippy i formalny baseline waiver

Wykonano:

```text
cargo clippy -p ghost-brain -p ghost-launcher --lib --tests --quiet --message-format=short -- -A clippy::never_loop -A clippy::absurd_extreme_comparisons
```

Wynik: `PASS` dla kompilacji zmienionych crate’ów. Dwa wyciszenia dotyczą
wyłącznie istniejących testowych diagnostyk poza diffem PR2.

Pełna literalna komenda z planu:

```text
cargo clippy -p trigger -p ghost-brain -p ghost-launcher --all-targets -- -D warnings
```

nie jest obecnie zielona już na bazowym SHA. Formalny waiver jest zawężony do
`5853f4fda2430b0101cab4dd38ee2e7cbcbee90f`, `rustc 1.95.0 (59807616e
2026-04-14)` oraz `cargo 1.95.0 (f2d3ce0bd 2026-03-21)` i do plików **poza
diffem PR2**, między innymi:

- `ghost-core/src/init_pool_parser.rs` — doc-comment / unused-variable;
- `ghost-core/src/shadow_ledger/*` — deprecated bootstrap compatibility;
- istniejące legacy/offline moduły `ghost-brain/src/oracle/*`;
- istniejące nietknięte moduły `ghost-launcher/src/components/*`.

Waiver nie obejmuje żadnej diagnostyki w zmienionych plikach PR2. Nie jest to
twierdzenie, że pełny workspace Clippy jest zielony; jest to jawne rozdzielenie
historycznego długu od walidacji tego diffu.
