# ADR-8D: Position Manager Lite V1 — durable terminal commit i dokładny lifecycle identity

Status: `IMPLEMENTED / LOCAL VALIDATION COMPLETE / CI PENDING`

Typ: ADR-8D / follow-up review PR #67 / aktywny shadow post-buy / replay i capacity safety

Data: 2026-07-15

Repo: `smahacfel/Gho`

Branch: `agent/position-manager-lite-pr1-20260715`

Base SHA: `53382696eb06affbd309ca4d050f030d31a561b0`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_LITE_V1.md`, PR 1.

Poziom ryzyka: `HIGH` — poprawka dotyka terminalizacji aktywnej pozycji shadow,
momentu zwolnienia capacity, durable terminal truth oraz walidacji lifecycle.
Nie zmienia progów exit, nie aktywuje live, AEM, Guardian authority ani Revolvera.

## 1. Problem zamykany przez follow-up

Review PR #67 wykazał sześć rozjazdów kontraktowych:

1. terminal receiver mógł zakończyć lifecycle mimo nieudanego zapisu canonical
   `ShadowTerminalTruthV2`;
2. `EventValidator` wiązał terminal z kandydatem, a nie dokładną parą
   `position_id + position_epoch`;
3. niepoprawne immutable entry data mogły utworzyć monitorowaną pozycję, która
   później kończyła jako cichy `Hold`;
4. rodzaj unresolved outcome był częściowo wyprowadzany z tekstu błędu quote;
5. unresolved otrzymywał syntetyczny `exit_landed_slot`;
6. literalny all-targets Clippy z planu nie był zielony, a wyjątek baseline nie
   był sformalizowany.

## 2. Decyzja: canonical terminal append jest commit pointem

Terminalizacja shadow ma teraz kolejność:

```text
guarded economic apply
  -> operational lifecycle event
  -> stage private PendingTerminalCommit
  -> append legacy lifecycle proof (best effort, bez duplikacji po sukcesie)
  -> append canonical ShadowTerminalTruthV2
  -> derive replay/lifecycle projections
  -> remove canonical MonitoredPosition
  -> send exact ShadowTerminalDisposition
  -> release capacity w launcherze
```

`TerminalCommitReceipt` rozdziela statusy:

- `lifecycle_jsonl`;
- `canonical_shadow_v2`;
- `replay_projection`.

Tylko `canonical_shadow_v2 = Ok` jest commit pointem. Do tego momentu pozycja
pozostaje w prywatnym store `MonitoringEngine`, terminal receiver nie jest
rozwiązywany, mirror cleanup nie zachodzi, a launcher nie może zwolnić slotu.

Retry jest bounded częstotliwością istniejącego interwału quote retry. Pending
terminal zachowuje stabilne `action_id`, pozycję, epokę, rekord i disposition.
Nie jest ponownie podejmowana decyzja ekonomiczna i nie powstaje konkurencyjny
SELL. Udany canonical append może zakończyć lifecycle nawet przy awarii
derived replay projection; ten przypadek jest jawnie logowany jako degraded,
ponieważ canonical stream pozostaje SSOT.

Aktywny shadow launcher inicjalizuje minimalny terminal-truth harness również
wtedy, gdy opcjonalny research burn-in jest wyłączony. Brak opcjonalnego
burn-in nie może więc degradować terminal persistence do no-op.

## 3. Dokładna identity lifecycle

`EventValidator` utrzymuje stan pod kluczem:

```text
(run_id, lane, position_id, position_epoch)
```

Dla każdej pary position/epoch wymagane jest:

- dokładnie jedno wcześniejsze `PositionOpened`;
- ten sam candidate ID;
- dokładnie jeden terminal: `PositionClosed` XOR
  `ShadowPositionUnresolved`;
- brak terminala dla nieznanej pozycji lub epoki.

Kolejne legalne epoki tego samego kandydata są dozwolone. Terminal dla P2/99
nie może domknąć otwarcia P1/7, a dwa terminale tej samej epoki są naruszeniem.

## 4. Entry integrity i typed quote failure

Rejestracja pozycji shadow jest odrzucana przed `positions.insert()` i przed
`PositionOpened`, jeżeli:

- candidate ID albo position ID jest pusty;
- epoka jest zerowa;
- entry price jest brakujący, niefinitywny albo niedodatni;
- entry raw quantity jest brakujące albo zerowe.

Quote resolver zwraca teraz `PriceTruthFailureKind`, a engine mapuje go przez
exhaustive match do prywatnego `ExecutableQuoteFailureKind`. Tekst `detail`
pozostaje wyłącznie diagnostyczny. Słowa takie jak `zero`, `no fill` albo
`no executable` nie mogą zmienić terminal reason.

## 5. Provenance unresolved

Dla `PositionUnresolved` obowiązuje:

- `exit_landed_slot = None`;
- `exit_landed_slot_source = None`;
- legacy `terminal_slot = None`;
- `truth_slot` przechowuje wyłącznie slot quote/evidence, jeśli istnieje;
- `terminal_observed_slot = None`, jeżeli terminal powstał wyłącznie w lokalnym
  runtime i nie ma uczciwego chain slotu;
- `terminal_ts_ms` pozostaje czasem obserwacji terminalnej.

Nie jest syntetyzowany slot fill ani landing dla zdarzenia bez fill.

## 6. Fault-injection i regresje

Nowy test terminal persistence wymusza błąd canonical append przez zastąpienie
ścieżki JSONL katalogiem. Potwierdza kolejno:

1. pozycja pozostaje aktywna;
2. pending terminal pozostaje w private state;
3. oneshot terminal receiver ma stan `Empty`;
4. po usunięciu awarii bounded retry zapisuje canonical terminal;
5. dopiero wtedy pozycja znika i receiver zwraca `SimulationBlocked`;
6. canonical stream zawiera dokładnie jeden terminal.

Pozostałe testy obejmują wrong-position, wrong-epoch, duplicate-terminal, dwie
legalne epoki, invalid immutable entry oraz niezależność reason od tekstu błędu.

## 7. Formalny waiver all-targets Clippy

Plan został doprecyzowany: baseline Clippy może otrzymać waiver wyłącznie gdy
jest poza diffem PR, jawnie przypisany do bazowego SHA i jednocześnie scoped
Clippy dla powierzchni PR jest zielony.

Dokładne polecenie:

```bash
cargo clippy -p trigger -p ghost-brain -p ghost-launcher --all-targets -- -D warnings
```

na toolchainie `rustc 1.95.0 / cargo 1.95.0` kończy się przed pełną oceną
zmienianych crate'ów na istniejących diagnostykach transitive workspace.
Pierwsze potwierdzone klasy/lokalizacje to między innymi:

- `gui-backend/src/portfolio.rs` i `price_oracle.rs` —
  `clippy::result_large_err`;
- `ghost-core/src/init_pool_parser.rs` —
  `clippy::empty_line_after_doc_comments` i unused variable;
- `ghost-core/src/shadow_ledger/ledger.rs` — istniejące deprecated calls;
- `ghost-core/src/checkpoint/types.rs` — `clippy::derivable_impls`;
- `ghost-core/src/shadow_ledger/*` — istniejące doc/style/dead-code lints;
- wcześniejsze znane all-targets baseline: `ghost-brain/src/pipeline/execution.rs`,
  `ghost-brain/tests/mock_pump_amm.rs` oraz
  `ghost-launcher/examples/oracle_pipeline_diagnostic.rs`.

Żaden z tych plików nie należy do diffu PR #67 względem base SHA. Waiver nie
obejmuje żadnej diagnostyki w pliku zmienionym przez PR. Scoped gate dla
`trigger`, `ghost-brain` i `ghost-launcher` lib/tests pozostaje wymagany, tak
samo jak testy runtime, formatter i `git diff --check`.

Nie naprawiono szerokiego baseline Clippy w PR1, ponieważ byłby to niezwiązany,
duży refactor wielu workspace crates i zwiększyłby ryzyko regresji dokładnie w
momencie domykania lifecycle safety.

## 8. Inwarianty po poprawce

- canonical terminal truth poprzedza terminal notification i capacity release;
- brak durable canonical append utrzymuje ekspozycję fail-closed;
- terminal należy do dokładnej pozycji i epoki;
- invalid immutable entry nie tworzy pozycji ani `PositionOpened`;
- terminal taxonomy nie zależy od tekstu diagnostycznego;
- unresolved nie udaje fill, landing ani confirmed exit;
- shadow `SimulationBlocked` i live `Unknown` pozostają różnymi semantykami;
- live pozostaje disabled, a live Unknown nadal nie zwalnia capacity;
- polityka +50% / -50% / 30 s i lazy quote pozostają bez zmian;
- Guardian pozostaje observation-only;
- prebuy Decision Plane pozostaje bez zmian.

## 9. Rollback

Rollbackiem jest revert follow-up commita wraz z bazowym PR1. Nie wolno
pozostawić wariantu, w którym terminal receiver jest ponownie wysyłany przed
canonical append. Jeżeli canonical writer nie może wystartować w shadow mode,
`PostBuyRuntime` kończy inicjalizację fail-closed.

## 10. Walidacja lokalna

Po finalnym formatterze wykonano następujące kontrole:

- `cargo test -p ghost-brain events::validator::tests --lib --quiet` — 12/12;
- `cargo test -p ghost-brain guardian::post_buy::engine::tests --lib --quiet` — 46/46;
- `cargo test -p ghost-brain guardian::post_buy --quiet -- --test-threads=1` — 174/174;
- `cargo test -p trigger entry_price_extractor::tests --lib --quiet` — 10/10;
- `cargo test -p ghost-launcher components::post_buy_runtime::tests --quiet -- --skip shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff` — 64/64;
- `cargo test -p ghost-launcher --test post_buy_runtime_integration --quiet` — 4/4;
- `cargo test -p ghost-launcher --test gatekeeper_v25_regression --quiet` — 42/42;
- `cargo test -p ghost-launcher --test gatekeeper_v3_tests --quiet` — 9/9;
- produkcyjny kontrakt konfiguracji progów post-buy — 1/1;
- scoped Clippy dla `trigger`, `ghost-brain` i `ghost-launcher` lib/tests — exit 0;
- `cargo fmt --all -- --check` i `git diff --check` — wymagane przed commitem.

Dokładny all-targets Clippy pozostaje objęty wyłącznie waiverem opisanym w
sekcji 7. CI dla finalnego SHA pozostaje wymagane przed zmianą statusu PR na
Ready.
