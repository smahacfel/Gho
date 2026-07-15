# ADR-8D: Position Manager Lite V1 — terminal commit probe i provenance `truth_slot`

Status: `IMPLEMENTED / LOCAL VALIDATION COMPLETE / CI PENDING`

Typ: ADR-8D / follow-up review PR #67 / shadow probe lifecycle / terminal truth provenance

Data: 2026-07-15

Repo: `smahacfel/Gho`

Branch: `agent/position-manager-lite-pr1-20260715`

Base SHA: `53382696eb06affbd309ca4d050f030d31a561b0`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_LITE_V1.md`, PR 1.

Poziom ryzyka: `MEDIUM` — zmiana domyka terminalizację kontrfaktycznego
shadow probe i usuwa fałszywe provenance slotu. Nie zmienia polityki exit,
progów, Gatekeepera, konfiguracji live ani execution authority.

## 1. Problem

Po wprowadzeniu durable terminal commit aktywny `shadow_monitor` otrzymywał
`ShadowV2ValidationHarness`, lecz oddzielny `probe_monitor` nie. Każdy terminal
probe przechodził wtedy do `PendingTerminalCommit`, ale canonical append zwracał
`NotConfigured`. Pozycja pozostawała aktywna i kolejne ticki ponawiały tę samą
nieskuteczną próbę, aż monitor probe mógł wyczerpać limit pozycji.

Niezależnie od tego materializacja evidence kopiowała `pos.slot` do
`sample_slot`, gdy nie było exit snapshotu albo snapshot nie miał własnego
slotu. `pos.slot` jest provenance entry/handoff, więc terminal bez exit evidence
mógł fałszywie raportować `truth_slot` równy slotowi wejścia.

## 2. Decyzja: osobny durable terminal stream dla probe

Probe używa tego samego kontraktu terminal commit co aktywne shadow, ale zapisuje
go do osobnego katalogu:

```text
<events_output_path>/position_manager_probe_terminal_truth_v2/
  shadow_position_event_v2.jsonl
  shadow_replay_v2.jsonl
  shadow_lifecycle_v2.jsonl
  shadow_path_density_v2.jsonl
```

Separacja jest świadoma:

- terminal probe musi mieć durable canonical commit przed usunięciem pozycji;
- failure canonical append pozostaje fail-closed w `PendingTerminalCommit`;
- aktywne shadow i kontrfaktyczne probe nie współdzielą canonical streamu;
- strumień eksperymentalny nie może być omyłkowo uznany za aktywny outcome BUY;
- oba monitory zachowują ten sam lifecycle i retry semantics.

Jeżeli probe lifecycle jest skonfigurowany, ale jego harnessu nie można
zainicjalizować, `PostBuyRuntime` kończy preflight fail-closed. Nie uruchamia
monitora, który nie byłby zdolny do durable terminalizacji.

## 3. Decyzja: slot evidence nie dziedziczy slotu entry

Materializacja `PostBuyDecisionSnapshot` stosuje następujący kontrakt:

- snapshot z własnym slotem -> `sample_slot = snapshot.slot`;
- snapshot bez slotu -> `sample_slot = None`;
- brak snapshotu -> `sample_slot = None`;
- `pos.slot` pozostaje wyłącznie entry provenance;
- `truth_slot` może pochodzić tylko z rzeczywistego quote/evidence slotu;
- unresolved bez exit snapshotu nie ma `exit_sample_slot`,
  `exit_market_anchor_slot`, `exit_landed_slot`, `terminal_observed_slot` ani
  `terminal_slot`.

Timestamp lokalnej terminalizacji pozostaje dostępny osobno i nie jest
przedstawiany jako chain slot.

## 4. Test kontraktowy

Rozszerzony test probe wykonuje pełną sekwencję:

```text
probe handoff
  -> terminal trigger bez executable exit snapshotu
  -> canonical probe TERMINAL_TRUTH append
  -> active_position_count == 0
  -> dokładnie jeden TERMINAL_TRUTH
  -> kolejny probe dla tego samego minta może zostać przyjęty
```

Test potwierdza również, że legacy probe lifecycle nadal zapisuje entry slot,
ale wszystkie nieistniejące sloty exit są puste.

Test unresolved w `MonitoringEngine` materializuje terminal bez snapshotu i
wymaga jednocześnie:

- zachowania `entry_slot`;
- `truth_slot = None`;
- `sample_slot = None`;
- `exit_sample_slot = None`;
- braku synthetic exit landing.

## 5. Inwarianty

- durable canonical commit poprzedza terminal notification i cleanup probe;
- brak harnessu nie może stworzyć permanentnie działającego, lecz
  nieterminalizowalnego monitora;
- aktywny outcome stream nie zawiera kontrfaktycznych probe;
- entry provenance nie udaje exit truth;
- brak snapshotu nie jest fill ani resolved close;
- shadow `SimulationBlocked` pozostaje różne od live `Unknown`;
- lazy executable quote i kolejność SL -> TP -> inactivity pozostają bez zmian;
- Guardian pozostaje observation-only;
- live execution pozostaje disabled;
- prebuy Decision Plane pozostaje bez zmian.

## 6. Rollback

Rollbackiem jest revert tego follow-up commita. Nie wolno pozostawić wariantu,
w którym probe monitor jest uruchamiany bez canonical terminal writer albo
w którym brak exit evidence jest zastępowany slotem entry.

## 7. Walidacja

Lokalnie zakończono:

- targeted unresolved provenance test — 1/1;
- targeted probe terminal lifecycle test — 1/1;
- testy `MonitoringEngine` — 46/46;
- testy Guardian post-buy — 174/174;
- testy `PostBuyRuntime` — 64/64;
- integracja post-buy — 4/4;
- produkcyjny kontrakt konfiguracji progów post-buy — 1/1;
- zamrożone regresje Gatekeeper V2.5 — 42/42;
- zamrożone regresje Gatekeeper V3 — 9/9;
- scoped Clippy zgodnie z waiverem PR #67 — exit 0.

`cargo fmt --all -- --check` i `git diff --check` są wykonywane ponownie po
finalnej aktualizacji dokumentacji. Zielone wymagane GitHub Actions dla
finalnego SHA pozostają osobnym warunkiem oznaczenia PR jako Ready.
