# Baseline receipt: Ingest/State/Quote PR1A — selected baseline 88aa1b7

Status: `BASELINE SELECTED / CI GATES RECORDED / NON-CI CHANGE-SET-0 ITEMS DEFERRED HARD GATE`

Data: `2026-07-24`

Repo: `/root/Gho_ingest`

Branch roboczy: `agent/ingest-state-quote-boundary-20260724`

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_WYKONAWCZY_NAPRAWY_GRANICY_INGEST_STATE_QUOTE.md`

## Decyzja baseline

Review PR1A wskazało formalny konflikt: plan zamrażał
`a12ef9cfb7199d44841cde27be2ecd8af13e2f3f`, natomiast implementacja PR1A
powstała na późniejszym commicie
`88aa1b775d51f4a1b3e512b1aaf05663e7af6db1`.

Na potrzeby PR1A wybrano drugą ścieżkę z review: baseline planu został
zaktualizowany do:

```text
88aa1b775d51f4a1b3e512b1aaf05663e7af6db1
```

Ten receipt dotyczy wyłącznie tego wybranego baseline. Nie próbuje udowadniać
braku regresji względem starego `a12ef9c`.

## Warunki wykonania

Polecenia baseline wykonano w tym samym repozytorium wskazanym przez
użytkownika: `/root/Gho_ingest`. Nie użyto `/root/Gho`,
`/root/Gho_dynamic_exit_v1_pr2b`, żadnego innego klonu ani dodatkowego
worktree.

Przed uruchomieniem bramek PR1A diff został zapisany przez:

```text
git stash push -u -m codex-pr1a-before-baseline-88aa1b7-20260724
```

Następnie potwierdzono czysty baseline:

```text
git status --short
# brak outputu

git rev-parse HEAD
88aa1b775d51f4a1b3e512b1aaf05663e7af6db1

git branch --show-current
agent/ingest-state-quote-boundary-20260724
```

Po zakończeniu bramek stash został zastosowany z powrotem przez
`git stash apply --index stash@{0}`. Stash pozostaje jako recoverable backup
do czasu świadomego usunięcia.

Logi robocze z przebiegu baseline znajdują się w ignorowanym katalogu:

```text
target/pr1a_baseline_88aa1b7_logs/
```

## Hashe wejściowe baseline

`Cargo.lock`:

```text
3362b5fcd6cb305407906a9dbcd6a9094398ae1c7e6830d34ee4fb5c30c7a7c3  Cargo.lock
```

Pliki konfiguracyjne TOML z maksymalnej głębokości 2:

```text
493b53bae91cc50624c1588c8ad58739f9de00e7707a16e8d299bd2b5ac27348  .cargo/config.toml
71827165c21dccf1e591e6142b0b735914f3d85a7c2c97c5cc8f0d2e105a2d15  Cargo.toml
5d702a48a705486d0f6ec7c08ed157a8220d0b9ec94d6f6fccb8900b2420d4dd  config.toml
ba9f5e75dbfe5d7b312727a1aa7dc280e6883ac596efc4784bfe26965b518b44  configs/dual-micro-live.toml
04e2cbfb4b1df61cdd562b348f9e3abac5b5301bcadf15b137b36818ca0ea835  configs/future-live.toml
c9820be54d38ef4f930b47bdfe74513cc0356d89cd9e33fd9f21a9da5e0551bc  configs/paper-burnin.toml
ab981c2db3166802116551f738cb83e33fceb052e65aa8c17cb2133e1c067a1a  configs/shadow-burnin.toml
87276ab43cd32837a9ebfb9ebbbb607c0675f8174dda9d1da7269da967b6933f  ghost-brain/Cargo.toml
32d7dd3dd1f997a181cacedd59fcfd7c8837a2e999ce3c22489b1df8179cc74e  ghost-brain/ghost_brain_config.example.toml
81419c88b9a2e79589e02674899535f98ab47c227aef91baa81891d53c78e032  ghost-brain/ghost_brain_config.toml
ba1f165791e4eb68ce5c8f514c641b1c89e6dcfdcfd043feffba2eab4720e467  ghost-core/Cargo.toml
6ea0847e202e20f0319b76abed9add179b5b367557a141e61396f928c82cedae  ghost-launcher/Cargo.toml
1c07057a6e4e3608596d073fcfed8a7c86cbdb918fa43fde08f2b2a0e15ff015  gui-backend/Cargo.toml
7d21c66a15f41bad66b82ed06a289f5dc8a6e3751951d2696bbf20ce296f2b0f  rust-toolchain.toml
```

## Baseline CI gates

| Bramka | Wynik na czystym `88aa1b7` | Sygnatura / klasyfikacja |
|---|---:|---|
| `cargo fmt --all --check` | PASS, exit 0 | log pusty; formatowanie baseline zielone |
| `cargo test -p ghost-core` | FAIL, exit 101 | `ghost-core/tests/pr1_contracts_foundations.rs`, `foundational_types_serialize_and_deserialize_roundtrip`, `deserialize account update: InvalidTagEncoding(104)`; summary: `3 passed; 1 failed` |
| `timeout 300s cargo test -p seer` | FAIL/TIMEOUT, exit 124 | przed timeoutem baseline ma failure w `connect_geyser_live_transaction_*`, PumpPortal, WAL oraz zawieszenie `tests::test_ultrafast_mode_keeps_forwarding_trades has been running for over 60 seconds` |
| `cargo test -p trigger` | FAIL, exit 101 | `jito_client::*status_uuid*` x2: `ConfigError("Jito status polling requires status_uuid but none is configured")`; `transaction_builder::tests::test_presigned_transaction_size_validation`: `assertion failed: presigned.size_bytes < 700`; summary: `335 passed; 3 failed` |
| `timeout 300s cargo test -p ghost-launcher` | FAIL, exit 101 | test-only `PoolTransaction` fixtures nie inicjalizują pól `complete`, `real_sol_reserves`, `real_token_reserves` i dwóch innych pól; `could not compile ghost-launcher` dla testów `gatekeeper_pdd_tests` i `oracle_continuous_sampling` |
| `timeout 300s cargo test --workspace` | FAIL, exit 101 | ta sama klasa baseline compile failure w `ghost-launcher/tests/metric_contracts_pr2a_producers.rs`: E0063 dla `PoolTransaction`; workspace zatrzymuje się przed dalszymi testami |
| `timeout 900s cargo build --release --workspace` | PASS, exit 0 | `Finished release profile [optimized] target(s) in 10m 29s`; ostrzeżenie future-incompat dla `solana-client v1.18.26` |
| `git diff --check` | PASS, exit 0 | log pusty na czystym baseline |

Wniosek: czerwone bramki pakietowe i workspace są obecne już na wybranym
baseline `88aa1b7`. Nie są wystarczające do uznania pełnego workspace za
zielony, ale są formalnie zapisanymi sygnaturami baseline dla PR1A.

## Post-build `target/release` executable inventory

Po zielonym `cargo build --release --workspace` zapisano hashe wykonywalnych
artefaktów znajdujących się we współdzielonym `target/release`:

> To jest post-build inventory, a nie dowód, że każdy wymieniony plik został
> przebudowany dokładnie przez tę jedną komendę. Katalog target nie był
> izolowany. Dowodem zielonej bramki jest wynik komendy w tabeli CI; silniejszy
> dowód binarny wymagałby osobnego `CARGO_TARGET_DIR`.

```text
0f4c544673592ed24ae15dce804910aef0f2187574ef5f1335d3e56e110d95af  target/release/e2e-test-runner
c380edd95fecd2fa1cb52f591961185afb0ba2813edef86562beb59943c859d1  target/release/ghost-brain-backtest-qedd
cfd570403fbff672fbff9652509fd57dca5262d59e56347310f0882f9b868406  target/release/ghost-brain-calibrate-qedd
deb11567ce844f0d5f8d20c2d9d831fce7e2b82c6f2394d16ffef53059d98879  target/release/ghost-e2e-runner
f51ee76e3a00433b596be95c3a9822568fb09f04f668ae9d876c7d28b3439232  target/release/ghost-gui
82dc0325cdc55bfc22f80db5aeca4cc7849605ed7d9be504bceb5eed75addd63  target/release/ghost-launcher
0810ac7c99e8888030a1f809788dc2ae5c775733f855e65e125be9809006fef9  target/release/metric_contract_audit
0812074f2cf1fac537f4ebefa366cb784332799fad8482a5be4fe94280862c5d  target/release/perf-test
9c5396fb0e449b432981337d3127a0eb32cb28f210ccf2ad54388b13811218ce  target/release/pumpfun_collector
057f16b1c4c46e2b3787c44a3328271f4022b6c3f1450ddca240547d7b5d31b5  target/release/replay_equivalence_proof
9a0c3d205672a2bc7c7a4db42082fd685a382a85100bf63cba076e0ca8fa2cf5  target/release/seer
cb609bfc0c7fe079ad90df91b9c3b6d4249d4a5b6e1beb5aa7897ae16b8f03e4  target/release/trigger
28847d4c6f5525ceb703e0e5161f41476366032942b7ddc5407f9577d1c2aa4c  target/release/v3_replay
```

## Non-CI Change Set 0 items

Plan wymaga także zapisania:

- canonical replay checksum;
- throughput;
- p99 receive-to-normalize;
- steady-state RSS;
- queue high-water marks;
- frozen differential corpus dla raw-only, raw+NLN duplicate, raw/NLN
  conflict, wielu Pump mutations w jednej signature, create+initial buy,
  account duplicate, same-version/different-hash conflict, `write_version=None`,
  queue saturation, writer stall, BuyV2 golden fixture, LegacySell golden
  fixture i missing anchor.

Ten checkout nie zawiera odnalezionego, planowo-kanonicznego harnessu ani
gotowego korpusu dla powyższej listy. Sprawdzenie repo wykazało tylko ogólny
`ghost-brain` proof:

```text
ghost-brain/docs/REPLAY_EQUIVALENCE_PROOF.md
ghost-brain/scripts/run_replay_equivalence_v2.sh
ghost-brain/src/bin/replay_equivalence_proof.rs
```

Ten proof porównuje `live_only` z `dual(lane=live)` na fixture'ach
`ghost-brain/tests/fixtures/replay/` i nie obejmuje wymaganej przez ten plan
granicy ingest/state/quote, providerów raw/NLN, account duplicates, queue
saturation ani fixture'ów BuyV2/LegacySell. Uruchomienie go nie byłoby
dowodem Change Set 0 dla PR1A.

Klasyfikacja formalna:

| Element | Status | Uzasadnienie |
|---|---|---|
| canonical replay checksum ingest/state/quote | DEFERRED HARD GATE | przed 1C/1D powstaje corpus raw/NLN/account conflict zgodny z planem |
| frozen differential corpus | DEFERRED HARD GATE | przed 1C/1D trzeba zamrozić corpus dla account duplicates, provider conflicts i raw/NLN reconciliation |
| throughput | DEFERRED HARD GATE | przed pierwszą zmianą zachowania transportu 1B ten sam harness/workload uruchamia się na rodzicu 1A i diffie 1B |
| p99 receive-to-normalize | DEFERRED HARD GATE | przed commitem 1B trzeba zapisać i porównać p99 tego samego workloadu |
| steady-state RSS | DEFERRED HARD GATE | przed commitem 1B trzeba zapisać i porównać RSS tego samego workloadu i czasu stabilizacji |
| queue high-water marks | DEFERRED HARD GATE | przed commitem 1B trzeba zapisać queue behavior/high-water dla tego samego workloadu |

To nie jest failure ukryty jako sukces ani trwały waiver. Dla addytywnego
PR1A CI część receipt dla wybranego baseline jest kompletna, a powyższe
elementy są formalnie odroczonymi twardymi bramkami: muszą zostać spełnione
przed określonymi turami, zgodnie z planem.

## Granica PR1A po kodowych poprawkach review

Kodowe blokery PR1A zamknięte przed tym receipt:

- brakujące konstruktory `AccountStateUpdate` i `SeerComponentConfig`
  uzupełnione;
- `GrpcConnection::connect_geyser()` waliduje kontrakt providerów
  synchronicznie przed przejęciem receiverów i przed `tokio::spawn`;
- `EntryAnchor` zachowuje `provider_id` oraz `provider_role`, także podczas
  konwersji z `PumpEvent::EntryUpdate`;
- aktywny `config.toml` jawnie deklaruje:

```toml
primary_raw_provider_id = "primary"
secondary_raw_provider_ids = []
```

Granica authority pozostaje bez zmian:

- nowe typy locator/order/provenance/provider role są addytywne;
- `provider_role` nie steruje `Gatekeeper`, `MaterializedFeatureSet`,
  canonical emission, shadow/live ani `AccountStateCore`;
- `txn_signature = None` pozostaje `None`;
- `tx_index = Some(0)` pozostaje pełnoprawnym indeksem;
- jedna signature może zawierać kilka poprawnych mutacji.

## Re-check finalnego diffu PR1A — review closure

Po zakończeniu baseline gates przywrócono diff PR1A i uruchomiono ponownie
kontrole. Poniższy re-check wykonano na finalnym diffie względem wybranego
baseline `88aa1b775d51f4a1b3e512b1aaf05663e7af6db1`. Wszystkie korekty review
PR1A — kontrakty początkowe, closure provenance/claims oraz captured payload
i replay pending update — są następnie squashed do jednego atomowego Commit
1A. Nie powstaje osobny commit review closure.

| Komenda | Wynik | Znaczenie |
|---|---:|---|
| `cargo fmt --all --check` | PASS, exit 0 | aktualny diff pozostaje sformatowany |
| `git diff --check` | PASS, exit 0 | brak whitespace errors |
| `cargo check -p ghost-launcher --test seer_connection_mode_test` | PASS, exit 0 | blocker E0063 dla `SeerComponentConfig` pozostaje zamknięty |
| `cargo test -p seer --lib connect_geyser_fails_closed_on_invalid_provider_role_contract -- --nocapture` | PASS, 1/1 | publiczna granica `connect_geyser()` zwraca błąd config przed spawnem |
| `cargo test -p seer --lib entry_anchor_preserves_provider_metadata -- --nocapture` | PASS, 1/1 | `EntryAnchor` zachowuje `provider_id` i `provider_role` |
| `cargo test -p seer --lib old_geyser_entry_anchor_json_defaults_provider_metadata -- --nocapture` | PASS, 1/1 | stare JSON `EntryAnchor` wczytują się bez provider metadata |
| `cargo test -p ghost-launcher --test seer_connection_mode_test -- --nocapture` | PASS, 7/7 | stary i jawny config source mode launchera nadal działają |
| `cargo check -p ghost-brain --tests` | PASS, exit 0 | wszyscy konsumenci addytywnych pól `AccountStateUpdate` kompilują się |
| `cargo test -p ghost-core ingest_integrity -- --nocapture` | PASS, 8/8 | locator/order/claims/hash i aliasy serde zachowują kontrakt; `tx_index = 0` nie znika |
| `cargo test -p seer --lib account_update_preserves_provider_and_optional_transaction_signature -- --nocapture` | PASS, 1/1 | `txn_signature = Some` i `None` oraz account provenance przechodzą przez IPC |
| `cargo test -p seer --lib test_account_update_uses_curve_mapping -- --nocapture` | PASS, 1/1 | account update nadal używa istniejącego mapowania curve |
| `cargo test -p seer --lib account_update_before_mapping_replays_provider_provenance_and_transaction_signature -- --nocapture` | PASS, 1/1 | `provider_id`, `provider_role` i `txn_signature` przechodzą przez `PendingCurveUpdateSnapshot` i replay IPC |
| `cargo test -p seer --lib raw_transaction_provider_metadata_reaches_parsed_trade_and_ipc -- --nocapture` | PASS, 1/1 | `PumpEvent::Transaction -> GeyserEvent -> TradeEvent -> SeerEvent::Trade` zachowuje ID i rolę providera |
| `cargo test -p seer --lib old_trade_json_defaults_provider_metadata_to_none -- --nocapture` | PASS, 1/1 | stary JSON `TradeEvent` wczytuje addytywną provenance jako `None` |
| `cargo test -p seer --lib provider_metadata -- --nocapture` | PASS, 5/5 | provenance `EntryAnchor`, `InitializePoolEvent -> CandidatePool -> IPC` i trade pozostają kompletne |
| `cargo test -p seer --lib connect_geyser_fails_closed_on_invalid_provider_role_contract -- --nocapture` | PASS, 1/1 | niepoprawny kontrakt ról zwraca `ConfigError` przed spawnem |
| `timeout 900s cargo build --release --workspace` | PASS, exit 0 | po ostatniej korekcie finalny diff przeszedł pełny build w limicie; bezpośrednie inkrementalne powtórzenie zakończyło się `Finished release profile [optimized] target(s) in 1.04s`; tylko istniejące ostrzeżenie future-incompat dla `solana-client v1.18.26` |

## Status merge

Ten dokument usuwa poprzedni blocker „brak formalnego wyboru baseline” oraz
uzupełnia brakujące baseline wyniki CI dla:

- `trigger`;
- `ghost-launcher`;
- `workspace`;
- `release build`.

Nie-CI Change Set 0 nie blokuje już addytywnego PR1A, ale pozostaje
`DEFERRED HARD GATE`. Przed pierwszą zmianą zachowania transportu 1B wymagany
jest identyczny harness/workload dla rodzica 1A i diffu 1B z zapisanym
throughput, p99 receive-to-normalize, RSS oraz queue behavior. Przed 1C/1D
wymagany jest zamrożony corpus dla account duplicates, provider conflicts i
raw/NLN reconciliation. Nie rozpoczyna się 1B w ramach tego commita PR1A.

To nie jest waiver: nie-CI bramki są odroczonymi warunkami wejścia do zmian
zachowania transportu i reconciliacji, a nie dowodem pełnej runtime parity.
