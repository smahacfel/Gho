# ADR-8D: Shadow Lifecycle Timeline Anchors

Status: IMPLEMENTED / TARGETED_TESTS_PASSED
Typ: ADR-8D / shadow lifecycle audit schema / simulation timeline evidence
Data: 2026-06-21
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: addytywne pola osi czasu dla `shadow_lifecycle.jsonl` i `probe_shadow_lifecycle.jsonl`
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `docs/ADR/ADR_8D_SHADOW_LIFECYCLE_TIMELINE_ANCHORS_20260621.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w poprzednich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Uczynic moment symulowanego wejscia i wyjscia z pozycji jawnie audytowalnym w lifecycle JSONL, bez zgadywania znaczenia `entry_slot` i `sample_slot`.

Wymagania:
- Dodac jawne pola dla entry simulation RPC slotu.
- Dodac jawne pola dla market anchor slot/source/signature przy entry i exit.
- Dodac jawny czas ewaluacji reasonu wyjscia.
- Dodac jawny syntetyczny `exit_landed_slot` dla symulacji.
- Nie usuwac starych pol `entry_slot` i `sample_slot`, aby nie zerwac kompatybilnosci analiz.

## 2. Opis problemu - 3W2H

What:
Dotychczasowe `shadow_lifecycle.jsonl` i `probe_shadow_lifecycle.jsonl` nie mialy jednoznacznego pola odpowiadajacego na pytanie, w ktorym slocie symulacja przyjmuje wejscie oraz w ktorym slocie ewaluowane jest wyjscie. `entry_slot` bylo puste dla zwyklego shadow, a `sample_slot` opisywalo probke price-truth, nie wykonanie SELL.

Where:
- event `GhostEvent::PostBuySubmitted`
- shadow/probe handoff w `PostBuyRuntime`
- `PositionJoinMetadata`
- `ShadowLifecycleRecord`

Why:
Analizy Target/StopLoss/TimeStop i rozkladu PnL opieraly sie na symulacji. Bez jawnych anchorow slotowych nie bylo mozliwe rozdzielenie:
- slotu kontekstu RPC symulacji BUY,
- slotu probki rynku uzytej do zamkniecia,
- syntetycznego przyblizenia landed slotu w symulacji,
- potencjalnej przyszlej realnej sygnatury transakcji.

How:
Dodano addytywne pola:
- `entry_simulation_rpc_slot`
- `entry_market_anchor_slot`
- `entry_market_anchor_tx_signature`
- `entry_market_anchor_source`
- `entry_landed_slot`
- `entry_landed_slot_source`
- `exit_sample_slot`
- `exit_market_anchor_slot`
- `exit_market_anchor_tx_signature`
- `exit_market_anchor_source`
- `exit_reason_evaluation_ts_ms`
- `exit_landed_slot`
- `exit_landed_slot_source`

How many:
Zmiana dotyczy tylko shadow/probe lifecycle evidence. Nie zmienia Gatekeeper BUY policy, scoringu, progow Target/StopLoss/TimeStop, TX buildera, sendera ani live inclusion path.

## 3. Przyczyna zrodlowa

Root cause:
`ShadowBuySimulationEvent` posiadal `rpc_slot`, ale zwykly shadow handoff nie przenosil go do monitoring lifecycle jako osobnego pola. Probe wpisywalo `rpc_slot` do `buy_landed_slot`, co dawalo niejednoznaczny `entry_slot`. Wyjscie lifecycle uzywalo `sample_slot` z price-truth evidence, ale nazwa pola nie mowila, ze to market sample, a nie slot wykonania wyjscia.

Skutek:
- `entry_slot = null` w wielu shadow lifecycle rekordach.
- Brak jawnego rozroznienia symulacji od realnej transakcji.
- Ryzyko blednej interpretacji `sample_slot` jako exit slotu.
- Brak bezposredniego pola czasu, w ktorym zapadla ewaluacja reasonu `Target`, `StopLoss` lub `TimeStop`.

## 4. Strategia naprawy

Przyjeta strategia:
- Zachowac stare pola dla kompatybilnosci.
- Dodac nowe pola addytywnie i wypelniac je tylko danymi, ktore runtime faktycznie zna.
- Dla shadow/probe wpisywac `entry_simulation_rpc_slot` z `ShadowBuySimulationEvent.rpc_slot`.
- Dla entry market anchor uzyc tego samego RPC context slotu i zrodla `shadow_simulation_rpc_context`.
- Nie wpisywac `probe_id`, `decision_ts_ms` ani lokalnych identyfikatorow jako `*_tx_signature`.
- Dla syntetycznego landed slotu uzyc `slot + 1` i jawnie oznaczyc zrodlo jako syntetyczne.
- Dla exit market anchor uzyc `PriceTruthEvidence.slot` oraz `PriceTruthEvidence.source`.
- Dla `exit_reason_evaluation_ts_ms` uzyc czasu tworzenia lifecycle rekordu.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: event handoff
- `GhostEvent::PostBuySubmitted` dostal opcjonalne `entry_simulation_rpc_slot`.
- Dodano builder `with_entry_simulation_rpc_slot`.
- Aktywny shadow przekazuje `Some(shadow_event.rpc_slot)`.
- Probe przekazuje `Some(shadow_event.rpc_slot)`.

Zmiana 2: metadata pozycji
- `PositionJoinMetadata` dostal entry timeline fields.
- `PostBuyRuntime` enrichuje shadow/probe handoff o entry anchor przed rejestracja pozycji w `MonitoringEngine`.

Zmiana 3: lifecycle JSONL
- `ShadowLifecycleRecord` emituje nowe entry i exit pola przy:
  - `exit_filled`
  - `exit_blocked`
  - `position_closed`
- `exit_sample_slot` jest jawnym semantycznym odpowiednikiem dotychczasowego `sample_slot`.
- `exit_landed_slot` jest tylko syntetycznym przyblizeniem `exit_sample_slot + 1`.

Zmiana 4: test coverage
- Rozszerzono test shadow lifecycle economics o asercje entry/exit timeline fields.
- Rozszerzono test probe lifecycle path o asercje nowych pol w `probe_shadow_lifecycle.jsonl`.

## 6. Walidacja

Walidacja po implementacji:

| Walidacja | Oczekiwany wynik | Status |
|---|---|---|
| `cargo fmt --all --check` | no formatting diff | PASS |
| `cargo test -p ghost-brain --lib shadow_runtime_close_writes_economics_and_lifecycle_proof` | 1 passed | PASS |
| `cargo test -p ghost-launcher --lib probe_handoff_uses_isolated_probe_monitor_and_lifecycle_path` | 1 passed | PASS |
| `git diff --check -- ghost-brain/src/guardian/post_buy/engine.rs ghost-launcher/src/events.rs ghost-launcher/src/components/post_buy_runtime.rs ghost-launcher/src/oracle_runtime.rs docs/ADR/ADR_8D_SHADOW_LIFECYCLE_TIMELINE_ANCHORS_20260621.md` | no whitespace errors | PASS |

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: pomylenie syntetycznego slotu z realnym landed slotem.
- Zabezpieczenie: dodano pola `entry_landed_slot_source` i `exit_landed_slot_source`, a dla shadow/probe wartosci syntetyczne sa opisane jako `synthetic_next_slot_*`.

Ryzyko 2: falszywa sygnatura transakcji.
- Zabezpieczenie: `entry_market_anchor_tx_signature` i `exit_market_anchor_tx_signature` pozostaja puste, dopoki runtime nie ma realnej on-chain signature.

Ryzyko 3: schema compatibility.
- Zabezpieczenie: zmiana jest addytywna. Stare `entry_slot` i `sample_slot` pozostaja bez zmian.

Ryzyko 4: shadow/live boundary.
- Zabezpieczenie: zmiana dotyczy tylko evidence/loggingu shadow/probe lifecycle. Nie wlacza live execution i nie zmienia sendera ani TX buildera.

## 8. Decyzja

Lifecycle JSONL musi rozdzielac:
- symulacyjny slot wejscia,
- market anchor wejscia,
- market sample wyjscia,
- czas ewaluacji reasonu,
- syntetyczne przyblizenie landed slotu.

Nowe pola sa obowiazkowym evidence surface dla dalszych analiz Target/StopLoss/TimeStop. Stare pola zostaja tylko jako kompatybilny legacy surface i nie powinny byc traktowane jako wystarczajacy opis osi czasu symulacji.
