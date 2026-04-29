# ADR-0120: Gatekeeper decision_log_path as SSOT

**Data**: 2026-04-30  
**Status**: Zaakceptowane  
**Autor**: GPT-5.4

---

## Cel zadania

Przywrócić deterministyczny zapis artefaktów Gatekeepera (`gatekeeper_v2_decisions.jsonl` i
`gatekeeper_v2_buys.jsonl`) do katalogu wskazanego przez `[oracle].decision_log_path`, bez
ukrytych override'ów runtime.

## Kontekst

Użytkownik zgłosił trzy sprzężone symptomy:

1. historyczne uruchomienia kończyły się na domyślnym `grpc_endpoint=http://localhost:10000`,
   więc rootowe `logs/system.log.*` zawierały tylko fail-fast startup;
2. rollout `shadow-burnin` pisał logi systemowe/oracle do `logs/rollout/shadow-burnin/*`,
   ale artefakty Gatekeepera nie pojawiały się tam, gdzie wskazywał config;
3. istniało podejrzenie, że BUY-e nie występują, bo brakowało oczekiwanego
   `gatekeeper_v2_buys.jsonl`.

Analiza świeżych artefaktów wykazała, że BUY verdicts realnie występowały, lecz były zapisywane
do błędnej, twardo zakodowanej ścieżki:

`logs/decisions.json/rollout/shadow-burnin/decisions`

Zatem główny regres nie leżał w polityce BUY-only loggera, tylko w rozjechaniu ścieżek między
runtime a konfiguracją.

## Decyzja

Przyjmujemy, że:

1. `[oracle].decision_log_path` jest jedynym źródłem prawdy dla katalogu artefaktów Gatekeepera
   w runtime launchera.
2. `DecisionLoggerConfig::default()` nie może zawierać rollout-specyficznej ścieżki, bo taki
   default przenosi zachowanie środowiskowe do warstwy bibliotecznej i maskuje błędy routingu.
3. Przykłady i ścieżki demonstracyjne muszą używać tego samego kontraktu co produkcja:
   gatekeeper verdict logs lądują pod skonfigurowanym rootem loggera decyzji.

## Wprowadzone zmiany

### 1. `ghost-launcher/src/oracle_runtime.rs`

- usunięto hardcode `logs/decisions.json/rollout/shadow-burnin/decisions`,
- `gatekeeper_log_dir` jest teraz ustawiany z przekazanego `decision_log_path`.

### 2. `ghost-brain/src/oracle/decision_logger.rs`

- zmieniono `DecisionLoggerConfig::default()`, aby `gatekeeper_log_dir` domyślnie był równy
  `log_dir`,
- doprecyzowano komentarz kontraktowy,
- dodano test pilnujący, że default nie odjedzie znowu od kanonicznego rootu decyzji.

### 3. `ghost-brain/examples/oracle_decision_dry_run.rs`

- przykład wyrównano do tego samego SSOT ścieżek.

## Konsekwencje

### Pozytywne

- `gatekeeper_v2_decisions.jsonl` i `gatekeeper_v2_buys.jsonl` trafiają tam, gdzie wskazuje
  config rolloutu;
- brak BUY pliku przestaje być mylony z brakiem BUY verdictów, jeśli BUY-e faktycznie istnieją;
- konfiguracja operatora odzyskuje pełną kontrolę nad lokalizacją artefaktów.

### Negatywne

- tooling lub ad hoc skrypty odwołujące się do błędnej historycznej ścieżki
  `logs/decisions.json/rollout/shadow-burnin/decisions` muszą zostać przełączone na ścieżkę
  konfiguracyjną;
- stare artefakty pozostają w poprzednim katalogu i mogą wymagać ręcznego posprzątania.

## Walidacja

1. uruchomić testy loggera / runtime,
2. potwierdzić, że nowe rekordy pojawiają się pod:
   `logs/rollout/shadow-burnin/decisions/`,
3. potwierdzić, że BUY-only plik powstaje przy pierwszym `decision_verdict_buy=true`.
