# ADR-8D: Profil i uruchomienie PREDATOR v11 shadow burnin

**Data:** 2026-09-16

**Status:** SUPERSEDED / SHADOW-ONLY RUN STOPPED

**Task:** `PREDATOR_V11_SHADOW_BURNIN_PROFILE_AND_LAUNCH_20260916`

## D0. Decyzja

Utworzono odrębny profil Ghost Brain PREDATOR v11 oraz odrębny profil
launchera dla pełnego shadow burnin. Runtime działa w sesji tmux
`ghost-predator-v11-20260916` z konfiguracją:

```text
configs/rollout/predator-v11-shadow-burnin-20260916.toml
→ configs/rollout/ghost_brain_predator_v11_shadow_burnin_20260916.toml
```

Tryb wykonania pozostaje `shadow`, a `trigger.entry_mode` pozostaje
`shadow_only`. DOW został wyłączony, aby `max_wait_time_ms = 2111` nie był
sprzeczny z jego rozszerzonym oknem 10 s. Nie włączono live send ani funding
lane.

## D1. Problem

Run wymagał nowego, odseparowanego profilu z progami PREDATOR v11 podanymi
przez operatora, bez zmiany pozostałych wartości profilu bazowego. Wartość
`max_wait_time_ms = 2111` nie spełniała aktywnego kontraktu DOW wymagającego
10 s. Operator jawnie zdecydował o wyłączeniu DOW.

## D2. Zakres konfiguracji

Profil Ghost Brain powstał przez skopiowanie aktywnego profilu bazowego i
zmianę wyłącznie wskazanych progów oraz `gatekeeper_v2.dow.enabled = false`.
Profil launchera powstał z kanonicznego profilu `shadow-burnin.toml` i używa
oddzielnych ścieżek logów oraz danych dla identyfikatora
`predator-v11-20260916`.

Hashy uruchomionych konfiguracji:

```text
ghost_brain_predator_v11_shadow_burnin_20260916.toml
  sha256 aac09e3b930e5921789d112f9836bcba0b71348545536bf0690814dac2e06ca6
predator-v11-shadow-burnin-20260916.toml
  sha256 5fa94aacc22fd15cbd276918e1ec862e30cd8ba8e6c4860682bf04038f7fdbd9
```

## D3. Uruchomienie

Binarkę `target/release/ghost-launcher` uruchomiono przez tmux z
`GHOST_ENV_FILE=/root/Gho/.env`. Sekrety nie zostały skopiowane do profilu ani
do tego dokumentu. Standardowe wyjście procesu jest dopisywane do:

```text
logs/rollout/predator-v11-20260916/launcher.stdout.log
```

## D4. Weryfikacja przed uruchomieniem

Wykonano:

```text
parser TOML i porównanie wszystkich wartości z profilem żądanym przez operatora
git diff --check
cargo test -p ghost-brain --lib ghost_brain_config::tests --no-fail-fast
cargo build --release -p ghost-launcher --bin ghost-launcher
ghost-launcher --config configs/rollout/predator-v11-shadow-burnin-20260916.toml --preflight
```

Testy konfiguracji zakończyły się wynikiem `45 passed, 0 failed`. Build oraz
preflight zakończyły się powodzeniem. Preflight potwierdził shadow-only,
łączność aplikacyjną z gRPC i RPC, dostępność salda technicznego, zapisywalne
ścieżki oraz wolne porty.

## D5. Dowody zapisu shadow burnin

Po uruchomieniu potwierdzono ciągły przyrost plików systemowych, Oracle,
stdout i JSONL. Powstały oraz rosły:

```text
logs/rollout/predator-v11-20260916/system.log.2026-09-16
logs/rollout/predator-v11-20260916/oracle.log.2026-09-16
logs/rollout/predator-v11-20260916/decisions/seer_runtime_coverage_audit.jsonl
logs/rollout/predator-v11-20260916/decisions/predator-v11-20260916/
  v2.2/.../gatekeeper_v2_decisions.jsonl
  v2.2/.../selector_shadow_score_v1.jsonl
  v2.5/coordination_risk/coordination_risk_evidence.jsonl
```

W pierwszej kontroli zapisano 18 rekordów decyzji Gatekeepera, 18 rekordów
selector shadow score, 18 rekordów coordination-risk evidence i 16 rekordów
coverage audit. Aktualne decyzje były głównie
`TIMEOUT_PHASE1_INSUFFICIENT` / `TIMEOUT_PHASE1_NO_DATA`, z jednym
`HARD_FAIL_EXTREME_TOP3`.

Podczas końcowej kontroli powstały również wszystkie trzy warunkowe artefakty
shadow execution:

```text
logs/shadow_run/predator-v11-20260916-buys.jsonl
logs/shadow_run/predator-v11-20260916/shadow_entries.jsonl
logs/shadow_run/predator-v11-20260916/shadow_lifecycle.jsonl
```

Każdy zawierał po jednym poprawnym rekordzie JSON. Próba shadow buy została
zbudowana i przekazana do symulacji, ale symulacja zakończyła się
`InstructionError(3, Custom(6002))`, sklasyfikowanym jako
`quote_slippage_error / too_much_sol_required`. Rekord ma zatem status
`not_lifecycle_eligible`; nie ma pozycji, terminalnego exitu ani PnL. Writer
warunkowych artefaktów działa, natomiast realny zapis PnL będzie możliwy do
potwierdzenia dopiero po pierwszej skutecznej symulacji i dopuszczeniu pozycji
do lifecycle. Brak PnL dla odrzuconej symulacji nie jest utratą danych.

## D6. Jakość połączenia gRPC

Porównano najnowszy slot `BlockMeta` ze slotem `processed` zwracanym przez
skonfigurowany RPC w czterech kolejnych próbkach. Różnice RPC minus gRPC
wyniosły `0, -3, 0, 0`, a wiek czasu najnowszego bloku wynosił około
`1,5-1,7 s`. W obserwowanym zakresie 795 kolejnych `BlockMeta` nie było
pominiętych ani niemonotonicznych slotów. Watchdog raportował `CONNECTED`,
`reconnects=0` i wiek ostatniego zdarzenia gRPC `1-561 ms`.

Podejrzenie stałego opóźnienia o kilkadziesiąt bloków nie zostało
potwierdzone. Późniejszy audyt wykazał, że raportowane tu wcześniej wartości
`0,573-7,585 s` nie były opóźnieniem detekcji poola. Był to wiek sekundowego
`block_time` względem zegara hosta, obciążony kwantyzacją i dryfem zegarów.
W zatrzymanym runie właściwy czas lokalnego przetwarzania kandydatów wynosił
`0-10 ms`. Korektę metryki i twardy budżet 50 ms opisuje osobny ADR
`ADR_8D_GRPC_CANDIDATE_HANDOFF_50MS_SLO_20260916.md`.

## D7. Granice i ryzyka

Nie zmieniono kodu Rust, schematu JSONL, policy order, reason codes ani granicy
shadow/live. Nie obchodzono fail-closed `CandidateIntegrity` ani wymagań
primary raw coverage. Checkout zawierał wcześniejsze, niepowiązane zmiany;
nie zostały one zmodyfikowane ani włączone do zakresu tej decyzji.

Licznik Prometheus `seer_grpc_connection_status` pozostaje równy zero, mimo że
watchdog i przyrost zdarzeń potwierdzają aktywne połączenie. Inspekcja kodu
wykazała rejestrację tej metryki bez miejsca, które aktualizuje jej wartość,
dlatego nie stanowi ona wiarygodnego dowodu stanu połączenia w tym runie.

## D8. Stan końcowy

Proces został zatrzymany na polecenie operatora przed wdrożeniem pomiaru
monotonicznego i twardego budżetu 50 ms. Zachowano jego logi i dane.

```text
GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```
