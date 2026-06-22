# ADR-8D: R38 CPV threshold lowered to 0.01 and runtime restart

Status: IMPLEMENTED / RUNTIME_RESTARTED / SMOKE_PASS
Typ: ADR-8D / config threshold change / shadow-only rollout restart
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `main`
Commit/PR: not committed at report time
Zakres: R38 Gatekeeper threshold profile, CPV hard gate, shadow-only restart
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- `docs/ADR/ADR_8D_R38_CPV_THRESHOLD_001_RESTART_20260618.md`

Powiazane runy/logi/raporty:
- R38 rollout config: `configs/rollout/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1.toml`
- R38 brain config: `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- Restart runtime log: `reports/selector/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T214628Z_cpv001_restart/runtime.log`
- New decision hash after restart:
  `logs/rollout/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/088e5de278e43df754039a57c56e677e514277521eed8513c5302b1983628b4d/gatekeeper_v2_decisions.jsonl`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zmienic R38 `min_cpv_other_pool_activity` z `8.5` na `0.01` i uruchomic ponownie ten sam run na zaktualizowanym configu.

Rzeczywisty przebieg:
- Potwierdzono, ze wskazany prog istnieje tylko w R38/R37 brain configach na linii CPV.
- Zmieniono tylko R38 brain config.
- Zweryfikowano parsowanie TOML dla brain configu i rollout profile.
- Zatrzymano aktywna sesje tmux R38.
- Uruchomiono R38 ponownie w tmux `r38-threshold-probe-maxwait31100`.
- Potwierdzono nowy `config_hash` i nowy `brain_config_hash` w decision logs.

Odchylenia od planu:
Nie zmieniano R37. Nie zmieniano rollout wrappera R38. Nie zmieniano kodu ani innych progow.

## 2. Wykorzystane skills/sub-agenci

Nazwa:
`ghost-execution`

Powod uzycia:
Zmiana dotyczy aktywnego shadow-only Gatekeeper profile, decision logs i runtime restartu.

Zakres uzycia:
Ochrona shadow/live boundary, DecisionLogger auditability i rozdzielenia config threshold od zmian kodu.

Wynik:
Zmiana zostala ograniczona do jednego progu w R38 brain configu.

Ograniczenia:
Smoke po restarcie nie jest pelna walidacja jakosci kandydata ani finalnym coverage report.

## 3. Opis problemu - 3W2H

What:
R38 mial zbyt agresywny hard gate `min_cpv_other_pool_activity = 8.5`, co dawalo tylko 2 klasyczne BUY na kilka tysiecy decyzji przed restartem.

Where:
`configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`.

Why it matters:
Tak wysoki CPV gate dominowal odrzuty i blokowal oczekiwany threshold-probe profile.

How observed:
Przed zmiana `gatekeeper_v2_decisions.jsonl` pokazywal 4378 decyzji, z czego tylko 2 `BUY`; CPV byl jednym z glownych hard fail buckets.

How many / scale:
W snapshot przed restartem w R38 old hash `291e0848...` mial:
- `BUY = 2`
- `REJECT_HARD_FAIL = 1923`
- `TIMEOUT_PHASE1_INSUFFICIENT = 1960`
- `TIMEOUT_PHASE1_NO_DATA = 493`

Evidence:
`strict_top` przed zmiana zawieral `threshold:cpv_other_pool_activity` jako dominujacy reject bucket.

## 4. Przyczyna zrodlowa

Root cause:
R38 odziedziczyl CPV threshold `8.5`, ktory byl za ostry dla celu aktualnego runa.

Mechanizm bledu:
Hard gate CPV odrzucal rows zanim mogly przejsc do klasycznego BUY path.

Miejsce:
`min_cpv_other_pool_activity` w R38 brain configu.

Skutek:
Klasyczny `gatekeeper_v2_buys.jsonl` mial tylko 2 rows mimo tysiacy terminal decisions.

Dowod:
Po zmianie na `0.01` i restarcie nowy hash w pierwszej probce mial `BUY = 10/48`.

Odrzucone hipotezy:
- Blad zapisu `gatekeeper_v2_buys.jsonl`: odrzucone, bo plik byl zgodny z liczba realnych verdict `BUY`.
- Brak restartu po edycji configu: wyeliminowano przez restart i nowy hash.

## 5. Strategia naprawy

Przyjeta strategia:
Zmienic tylko R38 `min_cpv_other_pool_activity` z `8.5` na `0.01` i zrestartowac R38, aby runtime zaladowal nowy brain config.

Zakres ingerencji:
- Jeden prog w jednym R38 brain configu.
- Restart R38 tmux session.

Czego nie zmieniano:
- R37 config.
- Gatekeeper code.
- Execution/send path.
- P37 probe code.
- FSC config.
- Inne threshold fields.

Ryzyka:
- Prog `0.01` moze znaczaco zwiekszyc BUY acceptance; wymaga dalszej obserwacji.
- W smoke po restarcie pojawil sie jeden `GATEKEEPER BUY PATH FAILED: BUY token simulation returned zero tokens`, do pozniejszego audit.

Odrzucone alternatywy:
- Zmieniac R37 razem z R38: odrzucone jako poza dyspozycja.
- Start bez restartu: odrzucone, bo runtime nie przeladowuje TOML w locie.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- Co zmieniono: `min_cpv_other_pool_activity = 8.5` -> `0.01`.
- Dlaczego: odblokowanie CPV hard gate dla R38 threshold-probe.
- Efekt: po restarcie nowy decision hash zaczal emitowac wyzszy udzial `BUY` w pierwszej probce.

Zmiana 2:
- Plik/modul: runtime/tmux
- Co zmieniono: zatrzymano stara sesje R38 i uruchomiono ja ponownie na tym samym rollout profile.
- Dlaczego: runtime laduje config przy starcie.
- Efekt: nowe decyzje sa zapisywane pod nowym `config_hash = 088e5de278e43df754039a57c56e677e514277521eed8513c5302b1983628b4d`.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| TOML parse | `python3 -c 'import tomllib ...'` | oba configi parsuja sie poprawnie | PASS | `toml_ok` |
| Config value | `rg -n "min_cpv_other_pool_activity"` | R38 ma `0.01` | PASS | line 217 |
| Restart proof | decision logs | new `config_hash=088e5de2...`, new `brain_config_hash=61308b86...` | PASS | new hash directory |
| Runtime alive | `pgrep -af ...r38...` | proces `ghost-launcher` aktywny | PASS | PID `3377995` |
| Smoke decision impact | new hash sample | V2.2: `BUY = 10/48` | PASS | new hash decision summary |

Wniosek walidacyjny:
R38 zostal ponownie uruchomiony na zaktualizowanym configu; nowe decyzje nie korzystaja ze starego hash/config.

Ograniczenia walidacji:
Pierwsze 48 decyzji po restarcie to tylko smoke sample. Pelne acceptance i coverage wymagaja pozniejszego raportu z wiekszej probki.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: config hash separation
- Co zabezpiecza: rozroznienie decyzji sprzed i po zmianie CPV
- Kiedy sie aktywuje: DecisionLogger zapisuje nowy `config_hash`
- Jak przetestowano: potwierdzono hash `088e5de2...` i brain hash `61308b86...`
- Co pozostaje poza zakresem: finalna jakosc signal/selector

Guardrail 2:
- Typ: scoped config edit
- Co zabezpiecza: brak przypadkowej zmiany R37 lub kodu
- Kiedy sie aktywuje: edit dotyczy tylko R38 brain configu
- Jak przetestowano: `rg` po progach i TOML parse
- Co pozostaje poza zakresem: dalszy manual tuning progow

## Otwarte ryzyka / follow-up

- Monitorowac czy `GATEKEEPER BUY PATH FAILED: BUY token simulation returned zero tokens` powtarza sie po wiekszej probce.
- Policzyc BUY acceptance i probe coverage po co najmniej kilkuset nowych decyzjach na hash `088e5de2...`.
- Jesli CPV `0.01` przepuszcza za duzo smieci, nastepna zmiana musi byc jawna i oddzielnie zahashowana.
