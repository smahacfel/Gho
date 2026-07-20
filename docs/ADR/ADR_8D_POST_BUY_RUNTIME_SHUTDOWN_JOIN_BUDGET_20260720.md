# ADR-8D: PostBuyRuntime shutdown join budget dla aktywnego HET-PM V2

Status: `IMPLEMENTED / SHUTDOWN FIX`

Typ: ADR-8D / post-buy runtime / shutdown / validation run

Data: `2026-07-20`

Repozytorium: `/root/Gho_dynamic_exit_v1_pr2b`

Uwaga o szablonie: wskazany w globalnych instrukcjach plik
`/root/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym środowisku.
Dokument używa lokalnego układu D1--D8 stosowanego w repozytorium.

## D1. Problem

Run `shadow-het-pm-v2-authoritative-20260720-retry7` potwierdził poprawę quote
freshness dla ścieżki wymagającej executable quote:

- `quote_required_current_executable_resolution_rate = 0.9575107857`;
- `quote_required_stale_snapshot_rate = 0.0342528435`;
- `queue_full_drops = 0`;
- `writes_failed = 0`;
- `comparison_attempts = writes_succeeded = 19_240`.

Ten sam run ujawnił jednak problem shutdown:

```text
PostBuyRuntime shutdown soft drain elapsed; keeping direct handoff receiver open until Oracle producers finish
PostBuyRuntime shutdown join timed out after 30s; aborting task
Component shutdown completed with 1 failure(s) or forced abort(s)
```

Przyczyną był konflikt budżetów:

- `PostBuyRuntime` posiadał własny hard drain direct handoff:
  `POST_BUY_SHUTDOWN_DIRECT_DRAIN_HARD_MS = 35_000`;
- launcher abortował standardowe komponenty po:
  `COMPONENT_SHUTDOWN_JOIN_TIMEOUT = 30s`;
- aktywny HET-PM V2 profile nie korzystał ze ścieżki
  `shadow_v2_burnin.logging_only`, która miała osobny wydłużony join budget.

Efekt: launcher mógł abortować `PostBuyRuntime`, zanim runtime osiągnął własny
hard deadline i wykonał końcowy flush / health finalization.

## D2. Decyzja

Dodano osobny bounded join budget dla `PostBuyRuntime`:

```rust
const POST_BUY_RUNTIME_SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(60);
```

`component_shutdown_join_timeout("PostBuyRuntime", ...)` zwraca teraz:

1. `post_run_manifest_drain_timeout_ms + SHADOW_V2_POST_BUY_JOIN_MARGIN` dla
   dotychczasowej ścieżki `shadow_v2_burnin.enabled && logging_only`;
2. `POST_BUY_RUNTIME_SHUTDOWN_JOIN_TIMEOUT` dla pozostałych trybów
   `PostBuyRuntime`;
3. `COMPONENT_SHUTDOWN_JOIN_TIMEOUT` dla innych komponentów.

Nie skrócono direct handoff drain. Celem jest usunięcie fałszywego forced abortu
bez ponownego generowania shutdown-edge handoff noise.

## D3. Granice bezpieczeństwa

Zmiana nie wpływa na:

- live execution;
- decyzję BUY/REJECT;
- HET-PM V2 policy thresholds;
- quote resolving;
- capacity ownership;
- terminal ownership.

Zmiana dotyczy wyłącznie shutdown join budgetu w launcherze. Timeout nadal jest
twardo ograniczony i pozostaje krótszy niż zewnętrzny launcher backstop
`timeout --kill-after=120s` używany w validation runnerze.

## D4. Konfiguracja

Brak nowego pola konfiguracyjnego.

Decyzja jest stałą runtime, ponieważ problem wynika z niespójności dwóch
istniejących stałych shutdown, a nie z potrzeby strojenia profilu.

Rollback:

```rust
component_shutdown_join_timeout("PostBuyRuntime", ...)
    -> COMPONENT_SHUTDOWN_JOIN_TIMEOUT
```

Rollback przywróci jednak ryzyko abortu przy aktywnym direct handoff drain.

## D5. Implementacja

Zmieniony plik:

- `ghost-launcher/src/main.rs`

Dodano:

- `POST_BUY_RUNTIME_SHUTDOWN_JOIN_TIMEOUT`;
- routing `PostBuyRuntime` w `component_shutdown_join_timeout()`;
- test potwierdzający, że domyślny `PostBuyRuntime` ma własny, większy
  bounded join budget.

## D6. Testy

Wymagane wąskie testy:

```bash
cargo test -q -p ghost-launcher test_post_buy_runtime_default_join_budget_exceeds_direct_handoff_drain --lib
cargo test -q -p ghost-launcher test_post_buy_runtime_gets_shadow_v2_manifest_drain_join_budget --lib
```

## D7. Wpływ na aktualne runy

`retry7` pozostaje użyteczny do oceny quote freshness, ale jego writer health ma:

```text
shutdown_complete = false
writer_health_evidence_status = incomplete_or_inconsistent
promotion_evidence_available = false
```

Nie należy traktować `retry7` jako czystego końcowego runu lifecycle. Po tej
poprawce wymagany jest kolejny krótki run sprawdzający:

- brak `PostBuyRuntime shutdown join timed out`;
- `shutdown_complete = true` w HET writer health;
- utrzymanie niskiego `quote_required_stale_snapshot_rate`.

## D8. Ryzyka

`PostBuyRuntime` może teraz czekać do 60 sekund na shutdown zamiast 30 sekund.
Jest to świadomy bounded trade-off: wolniejszy, ale poprawny clean shutdown jest
lepszy niż forced abort po 30 sekundach, który niszczy końcowe evidence.
