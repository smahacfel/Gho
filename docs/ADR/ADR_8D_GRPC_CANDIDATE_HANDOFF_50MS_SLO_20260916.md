# ADR-8D: Monotoniczny budżet 50 ms dla przekazania kandydata z gRPC do IPC

**Data:** 2026-09-16

**Status:** IMPLEMENTED / VERIFIED / SHADOW RUN ACTIVE

**Task:** `GRPC_CANDIDATE_HANDOFF_50MS_SLO_20260916`

## D0. Decyzja

Wprowadzono osobny, opcjonalny budżet
`seer.grpc_candidate_handoff_slo_ms`. Profil naprawczy PREDATOR v11 ustawia
go na `50`. Pomiar używa jednej osi monotonicznej procesu: od odebrania
wiadomości przez adapter Yellowstone do przyjęcia `PoolDetected` przez
ograniczoną kolejkę IPC.

Kandydat z wynikiem większym niż 50 ms lub bez wiarygodnego znacznika odbioru
zostaje zdegradowany do `Suppressed`. Surowa obserwacja pozostaje zapisana jako
evidence, ale pool nie może otworzyć spóźnionej sesji obserwacyjnej.

## D1. Problem i korekta diagnozy

Poprzedni raport nazwał opóźnieniem detekcji różnicę między zegarem hosta a
sekundowym polem Solana `block_time`. To pole nie daje pomiaru transportu:
ma rozdzielczość jednej sekundy i używa innego zegara. W 544 detekcjach
zatrzymanego runa ta diagnostyka miała zakres `573-17637 ms`, medianę
`2599,5 ms`, p95 `10819 ms` i p99 `16382 ms`.

Rzeczywisty, istniejący pomiar lokalnego przetwarzania kandydata dla tych samych
544 detekcji miał minimum `0 ms`, medianę `4 ms`, p95 `6 ms`, p99 `8 ms` i
maksimum `10 ms`. Luka polegała na braku end-to-end pomiaru od granicy odbioru
Yellowstone oraz braku wymuszonego limitu przed otwarciem sesji.

## D2. Zakres implementacji

- `SeerConfig` i launcher przyjmują opcjonalne
  `grpc_candidate_handoff_slo_ms` z `#[serde(default)]`, więc starsze profile
  zachowują poprzednie zachowanie.
- znacznik `ObservationProvenanceV1.received_at_monotonic_ns` jest porównywany
  z monotonicznym zegarem procesu przy przekazaniu kandydata;
- `seer_grpc_to_candidate_latency_ms` zapisuje rozkład opóźnienia;
- `seer_grpc_to_candidate_slo_breach_total` klasyfikuje `over_budget` i
  `missing_receive_timestamp`;
- legacy `seer_mint_to_detection_ms` i `seer_late_detection_total` zostały
  jawnie opisane jako diagnostyka wieku `block_time`, a komunikat logu został
  przemianowany na `BLOCK_TIME_AGE_DIAGNOSTIC`;
- profil naprawczy używa osobnych katalogów z identyfikatorem
  `predator-v11-20260916-latencyfix-r2`; zatrzymana próba diagnostyczna z 16
  workerami pozostaje w `predator-v11-20260916-latencyfix`.
- pierwszy diagnostyczny start wykazał dwa prawdziwe przekroczenia: `59,895 ms`
  i `138,473 ms`. Sam parser CREATE zajmował wtedy `3-4 ms`; opóźnienie powstało
  przed wejściem workera podczas burstu transakcji. Profil ustawia dlatego
  `event_worker_concurrency = 32` zamiast wyliczonego domyślnie limitu 16.

## D3. Zachowane kontrakty

Zmiana nie modyfikuje progów Gatekeepera, okna `2111 ms`, DOW, schematu
JSONL ani granicy shadow/live. Naruszenie budżetu jest fail-closed dla
admission, ale zachowuje raw observation na ścieżce dowodowej. Domyślna wartość
`None` nie zmienia innych profili.

## D4. Walidacja statyczna i testy

Wykonano:

```text
cargo fmt --all -- --check
git diff --check -- <pliki zakresu>
cargo test -p seer --lib grpc_receive_to_candidate_latency_uses_the_process_monotonic_axis
cargo test -p seer --lib grpc_candidate_handoff_slo_is_strict_and_fail_closed_when_unmeasurable
cargo check -p seer --lib
cargo check -p ghost-launcher --lib --bin ghost-launcher
cargo build --release -p ghost-launcher --bin ghost-launcher
ghost-launcher --config configs/rollout/predator-v11-shadow-burnin-20260916-latencyfix.toml --preflight
```

Oba testy jednostkowe zakończyły się `1 passed, 0 failed`. Check bibliotek i
binarki, build release oraz preflight zakończyły się powodzeniem. Pełny
`cargo check -p seer -p ghost-launcher --all-targets` pozostaje zablokowany
przez wcześniejszy, niezwiązany fixture
`temporal_carry_forward_contract_tests.rs`, który nie zawiera nowych pól
`PoolTransaction` (`complete`, `real_sol_reserves`, `real_token_reserves` i
dwa dalsze pola).

Hashy uruchomionych artefaktów:

```text
predator-v11-shadow-burnin-20260916-latencyfix.toml
  sha256 93668a16532881dd4134ce7fa5ca3410475d499de98673affea4ee1f5cd0be1c
ghost_brain_predator_v11_shadow_burnin_20260916.toml
  sha256 aac09e3b930e5921789d112f9836bcba0b71348545536bf0690814dac2e06ca6
target/release/ghost-launcher
  sha256 05aac8d5d28fe08390fc08502a74e12f97329579cd26663111b9fccb6ac57655
```

## D5. Walidacja runtime

Pierwsza próba z domyślnymi 16 workerami została zatrzymana po wykryciu dwóch
naruszeń na 80 pool-init: `59,895 ms` i `138,473 ms`. Oba poole zostały zgodnie
z kontraktem zdegradowane do evidence-only przed otwarciem sesji.

Druga, czysta próba R2 z 32 workerami objęła 101 pool-init: 88 Pump.fun i 13
PumpSwap. Wynik:

```text
seer_grpc_to_candidate_latency_ms_bucket{amm_program="pumpfun",le="50"} = 88
seer_grpc_to_candidate_latency_ms_bucket{amm_program="pumpfun",le="+Inf"} = 88
seer_grpc_to_candidate_latency_ms_bucket{amm_program="pumpswap",le="50"} = 13
seer_grpc_to_candidate_latency_ms_bucket{amm_program="pumpswap",le="+Inf"} = 13
seer_grpc_to_candidate_slo_breach_total = 0
```

Dla 88 pooli dopuszczonych do sesji: minimum `3,446 ms`, mediana `7,826 ms`,
p95 `26,450 ms`, p99 `36,343 ms`, maksimum `44,812 ms`. Najnowszy `BlockMeta`
i RPC `processed` wskazywały ten sam slot, więc `rpc - grpc = 0`.

W ciągu trzysekundowej kontroli rosły system log, Oracle log, decyzje Gatekeepera
i event dataset. Zweryfikowano poprawny JSON w 5 rekordach każdego z trzech
artefaktów decyzji, 5 rekordach coverage audit i 197 rekordach event dataset.
Pliki `shadow buy`, `shadow_entries` i `shadow_lifecycle` pozostawały warunkowo
nieutworzone, ponieważ R2 nie miał jeszcze shadow BUY. Nie ma więc jeszcze
pozycji ani PnL do zapisania; brak rekordu przed wejściem nie jest utratą danych.

## D6. Granica twierdzenia

Budżet obejmuje lokalny odcinek od adaptera Yellowstone do IPC. Nie mierzy
czasu od wykonania transakcji przez walidator do dostarczenia przez zewnętrznego
providera, ponieważ źródło nie dostarcza zgodnego znacznika monotonicznego z
zegarem hosta. Lag głowy strumienia należy nadal oceniać przez ciągłość slotów
i porównanie z bieżącym slotem RPC.

## D7. Stan procesu

Zweryfikowany run pozostaje w tle w tmux:

```text
session: ghost-predator-v11-latencyfix-r2-20260916
pid: 3166281
config: configs/rollout/predator-v11-shadow-burnin-20260916-latencyfix.toml
```

Po tej końcowej kontroli proces nie będzie dalej odpytywany ani restartowany bez
nowej dyspozycji operatora.

```text
GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```
