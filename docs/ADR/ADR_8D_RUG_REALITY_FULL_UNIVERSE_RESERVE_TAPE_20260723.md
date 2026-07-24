# ADR-8D: RUG reality-first full-universe reserve tape

Status: `RAW_TRADE_AND_ACCOUNT_RESERVE_PROPAGATION_IMPLEMENTED /
R1_R2_R3_R4_R5_R6_INVALID_PREFLIGHT /
OFFLINE_TRANSACTION_LOCAL_EXACT_REPLAY_FAIL /
REALITY_CAPTURE_BLOCKED`

Typ: ADR-8D / observe-only market-evidence correction

Data: `2026-07-23`

Repo: `smahacfel/Gho`

Branch: `agent/rug-scalp-v2-prospective-shadow-20260721`

## D0. Decyzja

`RUG_SCALP_SIGNAL_V2` pozostaje trwale odrzucony jako
`REJECTED_OVERCONSTRAINED`. Nie jest uruchamiany przez ten rollout i nie jest
baseline'em alpha.

Dodano wyłącznie pełno-universe, observe-only durable Pump tape. Każdy
zapisany `PoolTransaction` zachowuje raw post-trade canonical state z
dekodera Pump. Gdy canonical CPI trade nie niesie real reserves, writer może
użyć wyłącznie jednoznacznie złączonego raw Pump `AccountUpdate` z tym samym
post-trade `(slot, bonding_curve, virtual_sol_reserves,
virtual_token_reserves)`:

```text
virtual_sol_reserves
virtual_token_reserves
real_sol_reserves
real_token_reserves
complete
```

Order key i raw trade facts pozostają w tym samym rekordzie:
`slot`, `tx_index`, `event_ordinal`, `side`, `token_amount_units`,
`effective_curve_quote_lamports`, `mint` oraz `bonding_curve`.

## D1. Zakres

Zmiana jest addytywna i ograniczona do:

- durable schema/event emitter;
- full-universe transaction writer przed filtrowaniem per-pool;
- immutable manifestu runu z authority i profilem kosztowym;
- jednej konfiguracji observe-only;
- preflightu, który nie wymaga keypairu ani salda, gdy
  `trigger.enabled=false`.

Nie zmieniono `RugScalpSignalReducerV2`, progów, PM, BuyV2/LegacySell quote
math, fee-authority contractu, validation tape, Gatekeepera, Oracle policy,
transportu Yellowstone ani backfillu.

## D2. Kontrakt evidence

Pierwszeństwo ma raw post-trade Pump state, który parser otrzymuje wraz z
canonical trade eventem: virtual/real reserves oraz `complete`. To zachowuje
state właściwy dla konkretnego `slot/tx_index/event_ordinal`, zamiast
podmieniać go późniejszym odczytem konta. Tylko gdy raw event nie zawiera
pełnego state'u, bounded writer oczekuje maksymalnie 2 s na raw Pump
`AccountUpdate` o identycznym `(slot, bonding_curve, virtual_sol_reserves,
virtual_token_reserves)`. Kolizja trade'ów, powtórzona state update,
niedopasowany tuple, brak state'u lub przekroczenie TTL daje `None` — bez
wyboru po czasie ingressu, signature, cenie albo "ostatnim" stanie.

Gdy raw state i exact account join nie istnieją, durable row zachowuje `None`;
nie ma fallbacku do `AccountStateCore`, `ShadowLedger`, mark price,
`price_quote`, fixture'a ani `FEE_BPS=100`.

`effective_curve_quote_lamports` jest zapisywane tylko dla successful,
niesyntetycznego observed trade i tylko z raw `sol_amount_lamports`. Nie jest
rekonstruowane z ceny ani limitu instrukcji.

`Some(false)` dla `complete` jest zachowane jako prawidłowa wartość. Brak
`tx_index` dla backfillu nadal pozostaje non-evaluable; żaden timestamp,
ingress sequence ani signature nie zastępuje canonical order key.

## D3. Frozen cost/authority receipt

Przy starcie `rug_reality_capture` materializuje istniejący runtime
`BuyV2`/`LegacySell` fee authority z aktualnych canonical Pump accounts.
Manifest jest tworzony atomowo `create_new`, zawiera route IDs, schedule IDs,
evidence hash, code/config/binary hashes i pełny transaction-envelope profile.
Ponowne użycie namespace kończy się błędem zamiast nadpisania receiptu.

Konfiguracja r1 zapisuje aktualne policy values builderów: 400k CU po obu
stronach, entry fallback 25k micro-lamports/CU, exit 50k
micro-lamports/CU, entry tip 1_000_000, legacy sell Jito tip 300_000, base fee
5_000 oraz odczytane z private RPC rent exemption dla standardowego 165-byte
ATA: 2_039_280 lamports. Policy retry jest jawna: kosztem pierwszej próby,
a retry/failure cost pozostaje osobnym polem, nie ukrytym w program settlement.

## D4. Weryfikacja

Przechodzą:

1. `bridge_preserves_canonical_post_trade_pump_reserves`;
2. `full_universe_durable_row_preserves_canonical_reserves_and_order_key`;
3. `full_universe_durable_row_does_not_fallback_to_mark_price`;
4. dwa testy config/manifest `rug_reality_capture`;
5. `full_universe_reserve_join_uses_only_exact_post_trade_account_state`;
6. `full_universe_reserve_join_rejects_wrong_tuple_without_fallback`;
7. istniejące raw Yellowstone tests dla `index=0` i `index=37`;
8. `parsed_event_dedup_transfers_reserves_when_cpi_settlement_amounts_differ`;
9. `parsed_event_dedup_does_not_override_cpi_reserves_with_zero_instruction_enrichment`;
10. `cargo check -p seer -p ghost-launcher`;
11. `cargo fmt --check` oraz `git diff --check` przed startem capture;
12. rollout `--preflight` z private RPC, który materializuje oba aktualne
   execution-authorised schedules.

Preflight może jedynie sprawdzić config/transport/authority. Nie tworzy
manifestu runu, nie uruchamia streamu i nie dostarcza danych alpha.

## D5. Uruchomienie i stop rules

Pierwszy krótki preflight (`r1`) wykazał tylko 290/455 (63,7%) complete
reserve rows. Nie jest używany do discovery ani do oceny alpha. Przyczyną
było odrzucanie przez parser dostępnych już raw
`virtual_*_reserves`, `real_*_reserves` i `is_complete` przy konstrukcji
`TradeEvent`; nie był to brak danych u Yellowstone.

Drugi krótki preflight (`r2`) nadal nie przeszedł: 145/281 complete reserve
rows. Rozbicie pokazało, że wszystkie 136 brakujących rows były zachowanymi
`CpiTrade`. Deduplikacja poprawnie wybrała CPI jako canonical trade, ale
odrzucała jednoznacznie odpowiadający mu instruction trade z raw real reserve
state.

`r3` ujawnił drugą, wąską niespójność: 1 744/2 141 (81,46%) complete reserve
rows przy 100% complete order keys. Deduplikator wybiera CPI po
`(side, mint, canonical curve)`, natomiast transfer raw state wymagał także
zgodności `token_amount` i `sol_amount`. To są różne powierzchnie programu
(instruction cap / exact-out oraz settled event amount), dlatego 389 z 397
braków dotyczyło jedynie real reserves i `complete`, chociaż direct raw state
istniał w tej samej transakcji. `r3` jest nieważny technicznie i nie służy do
discovery.

`r4` używa identycznego, *unikalnego* klucza strukturalnego co deduplikacja:
`(side, mint, canonical curve)`. Gdy istnieje więcej niż jeden matching direct
trade, state pozostaje `None` (fail closed); nie wybieramy kolejności, czasu
ingressu ani podpisu jako zastępstwa.

`r4` nadal nie przeszedł: 1 050/1 218 (86,21%) complete reserve rows przy
100% canonical order keys. Wszystkie 168 brakujących rows miały virtual
reserves, lecz nie miały `real_sol_reserves`, `real_token_reserves` ani
`complete`; nie miały też pełnego durable trade row o tym samym signature.
Audyt kodu wykazał, że Seer dekodował te trzy pola z Pump `BondingCurve`, ale
usuwał je przy serializacji `CanonicalAccountUpdatePayload` /
`DetectedAccountUpdateEvent`. R4 nie jest używany do discovery. R5 propaguje
raw account state i łączy go wyłącznie exact tuplem opisanym w D2.

`r5` został zatrzymany po krótkim capture i jest nieważny: 2 928/2 928 live
successful rows miało canonical order key, ale 2 008 rows
otrzymało `virtual_sol_reserves=0` oraz `virtual_token_reserves=0` przy
`curve_data_known=true`. To nie były rzeczywiste zerowe rezerwy. CPI event
niósł własny virtual state (widoczny też w legacy diagnostic fields), ale
nowy transfer z instruction/meta path nadpisywał go zerowym sentinel'em
`enrich_trade`. R5 nie jest używany do discovery ani do oceny alpha.

R6 zmienia tylko tę semantykę: zero virtual tuple z instruction/meta path
jest `None`, a nie raw reserve evidence. CPI trade zachowuje własne canonical
virtual reserves i może zostać uzupełniony real reserves wyłącznie exact
account-update joinem z D2. Żadna cena, mutable cache ani syntetyczny order
nie bierze udziału w tym przejściu.

`r6` zakończył pełne 298,702 s bez silent stall. Capture zapisał 114 births,
6 079 successful live `grpc_global_stream` trades i 6 079/6 079 canonical
order keys. Fee authority był obecny w immutable manifeście. Pełny raw reserve
state miało jednak tylko 4 861/6 079 rows (79,9638%); 1 218 rows nie miało
co najmniej jednego z `real_sol_reserves`, `real_token_reserves`, `complete`,
zaś 740 nie miało również kompletnego virtual tuple. Dodatkowe 193 rows
nosiły `virtual_sol_reserves=0`, `complete=None` i pozostają non-evaluable.

To nie jest luka w `tx_index`, fee authority ani fallback writer'a. Na
rzeczywistym mintcie z r6 canonical account updates istnieją tylko dla
post-transaction/account-write state'u, podczas gdy jedna transakcja zawiera
kilka ordered Pump trade facts z różnymi virtual reserve tuples. Exact join
z D2 słusznie nie wybiera jednego późniejszego account state dla tych
pośrednich facts. Bez per-instruction canonical reserve state dostarczonego
przez źródło albo osobno udowodnionej exact state transition nie wolno
materializować ich real reserves. R6 nie jest używany do discovery, opportunity
replay ani oceny alpha; czterogodzinny capture nie został uruchomiony.

Rollout `configs/rollout/rug-reality-capture-20260723-r4.toml` ma
`trigger.enabled=false`, `rug_scalp_v2.enabled=false` oraz
`p37_shadow_probe.enabled=false`. Nie istnieje ścieżka build/submit/position
managera w tym runie.

Po release build startuje krótki preflight capture. Pełny segment może być
użyty do discovery wyłącznie, gdy nie ma silent stall powyżej 20 sekund,
niewyjaśnionej slot gap ani utraty fee authority. Segment jest zatrzymywany
na pierwszym z tych zdarzeń i nie jest sklejany przez lukę z następnym
segmentem.

Ten ADR nie autoryzuje nowej reguły, held-out tuning, Run A ani live
execution. Następny etap to wyłącznie exact offline label i opportunity
replay na kompletnych segmentach.

## D6. Reproducibility source set

`r4.code_hash` rollouta jest SHA-256 nad bazowym HEAD
`113c5aea19f4da873d21d0c513c9727a886cd270` i sumami plików:

```text
ghost-brain/src/events/schema.rs
ghost-brain/src/events/emitter.rs
ghost-brain/src/events/validator.rs
ghost-launcher/src/components/seer.rs
ghost-launcher/src/config.rs
ghost-launcher/src/events.rs
ghost-launcher/src/lib.rs
ghost-launcher/src/main.rs
ghost-launcher/src/oracle_runtime.rs
ghost-launcher/src/rug_reality_capture.rs
off-chain/components/seer/src/binary_parser.rs
off-chain/components/seer/src/ipc.rs
off-chain/components/seer/src/lib.rs
off-chain/components/seer/src/nln_program_streams.rs
off-chain/components/seer/src/pumpportal_connection.rs
off-chain/components/seer/src/types.rs
```

Wartość: `2af94387065df04749e0f252cdc9982f8f4cd11e48670cb342ef7daf90b320a2`.

R5 ma oddzielny immutable config
`configs/rollout/rug-reality-capture-20260723-r5.toml` i source hash:
`11b4bf89f878028146c406d8e6ed93c39b16bb50861fdbeef78b7e3fd5981d51`.
Nie nadpisuje receiptu ani namespace'u R4.

R6 ma oddzielny immutable config
`configs/rollout/rug-reality-capture-20260723-r6.toml` i source hash:
`97c1feb534aaf16fd189ac1217a971fb1d863e9ea1b7f61b63e196c25e1a9a50`.
Nie nadpisuje receiptu ani namespace'u R5.

## D7. Korekta bramki i offline transaction-local replay R6

Końcowa interpretacja D5 — że brak bezpośredniego AccountUpdate po każdej
instrukcji sam w sobie blokuje exact state — została zastąpiona prawidłową
bramką:

```text
>=99% EXACT PER-INSTRUCTION STATE
```

Offline-only `TRANSACTION-LOCAL EXACT PUMP STATE RECONSTRUCTION` grupuje
trades po `(slot, signature, bonding_curve)`, porządkuje po
`(tx_index, event_ordinal)`, zaczyna od pełnego final state na kanonicznie
ostatnim faccie i wykonuje reverse oraz forward replay. Nie wybiera stanu po
samym `slot + curve`, nie używa późniejszego snapshotu, ingress order, mark
price, ShadowLedgera ani interpolacji.

Na immutable R6 source hash
`sha256:507ced8ac1291d6985e2aa373a7d17f7f382a70dabbd2f916d652724408e9991`
wynik wynosi:

| Klasa | Rows |
|---|---:|
| `DIRECT_EXACT` | 3 340 |
| `RECONSTRUCTED_EXACT` | 232 |
| `NON_EVALUABLE_NO_ANCHOR` | 791 |
| `NON_EVALUABLE_UNKNOWN_MUTATION` | 4 |
| `NON_EVALUABLE_CONSERVATION_MISMATCH` | 1 519 |
| `NON_EVALUABLE_OTHER` | 193 |

`3_572 / 6_079 = 58.759664%` jest górnym limitem replayu na zachowanych
trade facts, nie pełnym exact coverage: R6 nie zachowuje manifestu wszystkich
curve-mutation instructions ani provenance state'u rozróżniającego local
direct state od suplementacji writer'a. Nie ma mismatchu forward/reverse ani
dostępnego virtual tuple, lecz występuje 1 519 bit-exact typed-transition
conservation mismatchów. Zatem R6 **nie przechodzi** bramki: targeted tests,
release build, live preflight i `RUG_REALITY_CAPTURE_1` nie są autoryzowane.

Reprodukcja jest zawarta w
`scripts/rug_reality_exact_state_audit.py`; maszynowy receipt w
`logs/rug_reality_capture/r6/exact_state_audit_v1.json`; raport review w
`PLANS/AUDYT/RAPORT_RUG_REALITY_R6_TRANSACTION_LOCAL_EXACT_STATE_20260723.md`.
