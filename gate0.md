# Gate0 — 2026-10-01: naprawa dev_volume_ratio po przerwanym runie10h

Pierwszy run10h gate0-1790810736215 zakończył się po6291s z dev_volume_ratio range. To unieważnia wynik tego runu jako pełnego datasetu; starszy odbiór1800 pozostaje historycznym technicznym PASS.

Odtworzono dokładny błąd przez silnik oraz Gate0 snapshot: BUY0.89/SELL0.37/BUY0.71 tego samego dev dawały1.0000000000000002 przez różny porządek sum w liczniku/mianowniku. Mianownik teraz korzysta z identycznego subtotal BUY+SELL. Kontrola[0,1] bez epsilona i clampowania; log błędu zawiera wartość/bity oraz mint/fazę. Dotychczasowy guard CI FTDI poprawiono po potwierdzeniu fałszywego zliczania nazw adaptera i producenta.

Walidacja i zakres: docs/ADR/ADR_8D_GATE0_DEV_VOLUME_RATIO_SUMOWANIE_20261001.md. Nowy run zachowa confirmed,Gem320,C/D/E,observe-only,admission10h+drain. Dokładny commit, binarka, PID i start w metadanych nowego runu; nie używać poniższych historycznych PID jako bieżącego stanu.

---

# Gate0 — publikacja i autoryzacja pierwszego runu 10 h

Użytkownik zatwierdził commit całego dorobku Gate0, push i nowy PR, następnie start10h. Bieżący odbiór1800s PASS jest opisany niżej. Nowy run ma admission10h + drain do600s, bez flagi smoke, observe-only i Gem320SOL. Po jednorazowym potwierdzeniu startu pozostawić bez ingerencji do powrotu użytkownika. Stan commita/PR i PID/run zapiszą metadane publikacji oraz runu; poniższe LOCAL ONLY i zakazy startu są historią sprzed tej autoryzacji.

ADR: docs/ADR/ADR_8D_GATE0_PUBLIKACJA_I_START_10H_20260930.md. Zmiany endpointów niezwiązane z Gate0 i lokalne notatki nie należą do PR. Binarka odbioru1800 pozostaje niezmieniona; dodatkowa poprawka wyłącznie testowa usuwa zależność asercji od kolejności HashMap.

---

# Gate0 — aktualizacja 2026-09-30, 21:58 UTC: pełne1800s PASS

Naprawiono profil źródła: Gate0 nie przenosi już dynamicznych kont wykonania BCV2/generic do subskrypcji obserwatora. Globalne krzywe/pule, transakcje i BlockMeta zachowane; ordinary PrimaryGlobal pozostaje dynamiczny. Lokalna retencja i guard10s bez zmian. Test produkcyjnego buildera/resub RED→GREEN; transport99/99,fmt/diff/clippy/releasePASS.

Pełne1800s PASS: start21:28:20UTC, koniec21:58:20.994441UTC, czas odrun_id1800.568s, exit0, reason=smoke_only, shutdown_error=null. Run gate0-1790803700426. Binarka1ce19530d144998e36f207c26d175cdc5307a27b5c1aca1fd5d66933bfa0ae74. Zero resubscribe, reconnectów, lokalnych dropów, pending mapping expiry i konfliktów statusów. Primary1844208/funding2320096 odebranych zdarzeń; konsument2491086. High-water1361/554 wobec16384, maksymalny lag266ms, końcowe kolejki puste.

1172births/terminali;1885snapshotów; fazy1155/376/342/8/4. TerminaleC765,D186,E149,completed4,smoke_only68. Cztery pełne obserwacje600s, trzy z migracją i initial AMM; łącznie14migracji i14initial. Wszystkie wymagania technicznego CLI smoke>=600s przeszły. Audyt60pól: zero naruszeń, known_creator1172/1172, identyczna populacja birth/terminal, brak powtórzonych snapshotów, poprawne prefiksy faz, newline-complete26362779b. CPV1133clean/361insufficient/391unavailable.

Gem0 ma konkretne przyczyny: jeden completed bez migracji, dwa z migracją w wieku3ms/7ms (warunek wymaga>3000ms), ostatni z migracją220344ms i minimumMC266.3246SOL<320. Jeden z wczesnych przypadków ma także historyczny missing_amm_state. Nie zmieniono żadnego progu.

Ograniczenia: log nadal zawiera1388 GATE0_TRADE_CANDIDATE_PARSE_MISS i jeden Missing pong. Po ostrzeżeniu ping125 o21:38:50 oba liczniki osiągnęły126/126 do21:38:55, bez przerwy danych, reconnectu lub fail guardu. Nie oznaczać ostrzeżeń parsera jako naprawionych. Ten run jest diagnostycznym smoke_only;68aktywnych obserwacji zamknięto na limicie. Skaner datasetu odrzuca go zgodnie z kontraktem jako smoke_only, więc accepted=false dla badania, przy source_smoke_pass=true i full_lifecycle_verified=true. To nie jest wynik badania ekonomicznego ani dowód stabilności10h. Nie dowodzi osobno wyłącznej przyczyny wszystkich historycznych przerw.

ADR: docs/ADR/ADR_8D_GATE0_DYNAMICZNE_KONTA_WYKONANIA_W_SUBSKRYPCJI_20260930.md. Artefakty: `/root/gho_gate0_runs_20260927/gate0_static_confirmed_1800s_20260930T212820Z.jsonl` i pliki towarzyszące. Żaden Gate0 nie jest aktywny. COMMITTED:nic; PUSHED:nic; PR:nie; wszystkie zmianyLOCAL ONLY. .env i endpoint bez zmian.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

---

# Gate0 — aktualizacja 2026-09-30, 19:22 UTC: naprawa keepalive

Run1800s z17:59 zakończył się po180.6s: primary progress unavailable/stale. Nie ma dowodu, że przyczyna była po stronie providera.

Naprawiono konkretny błąd klienta: ignorowany serwerowy Ping. Klient teraz odpowiada z kolejnym ID, a watchdog/keepalive mają priorytet przed danymi w biased select. Log transportu zachowuje health/ping/pong. Lokalny test prawdziwego gRPC: RED przed poprawką, GREEN po; grpc_connection98/98, CLI3/3, Clippy/fmt/diff/release PASS. Testy obu profili, bez zmian320SOL, confirmed, C/D/E, guardu10s i observe-only.

Próba release360s 19:16:15–19:22:16 UTC: exit0, reason=smoke_only, shutdown_error=null. Binarka6b4b325b1be30803dfce47fc52b7424caace8a226844991b96e39757bef045e6. Run gate0-1790795775836. Primary502165/funding592160 odebranych zdarzeń;616804 zdarzeń konsumenta; brak dropów, pending expiry, reconnectów i błędów postępu. Ostatnie health obu połączeń: ping70/pong70; recon=1 oznacza początkowe zestawienie, nie reconnect. Lag maks47ms, high-water635/593 wobec16384. 210births/terminali,312snapshotów, fazy192/68/50/2/0,4migracje+4initial; brak pełnego600s. Audyt60pól: zero naruszeń, developer210/210; CPV26clean/7insufficient/279unavailable. Zostało126 ostrzeżeń GATE0_TRADE_CANDIDATE_PARSE_MISS — nie przypisujemy im przyczyny bez dowodu ani nie oznaczamy ich jako naprawione. Tej próbki nie akceptujemy jako pełnego datasetu Gate0. Poprawka keepalive ma regresję RED→GREEN i lokalną walidację360s; dokładna przyczyna historycznego przerwania180s nadal nie została dowiedziona.

ADR: docs/ADR/ADR_8D_GATE0_IGNOROWANY_PING_GRPC_20260930.md. Artefakty: `/root/gho_gate0_runs_20260927/gate0_repaired_confirmed_360s_20260930T191615Z.jsonl` i pliki towarzyszące. Nie ma aktywnego Gate0. Wszystkie zmiany LOCAL ONLY; brak commit/push/PR. .env bez zmian.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

---

# Gate0 — stan po naprawie błędów 2026-09-30, 16:23 UTC

**Poprawki lokalne i testy PASS. Finalny source smoke60s PASS. Pełny lifecycle600s / gotowość10h: nadal NIEZWERYFIKOWANE. Próg fazyV/Gem pozostaje320 SOL.**

Naprawiono: utratę początkowego CreatePoolEvent przy swapie w tym samym TX (tylko opt-in Gate0), pomijanie samodzielnego zweryfikowanego CreatePoolEvent jako dowodu kanonicznej migracji, zachowanie starej ceny po błędnym stanie i utratę dokładnych przyczyn null. Gate0 jawnie używa source_commitment=confirmed; zmienia to moment dostępności danych względem dawnych processed runs. Sprzeczność statusów nadal fail-closed, przed mutacją CPV, z trwałym zapisem sygnatury i obu dowodów. CPV ma dokładne reason codes; warunki ready i wzory nie zostały osłabione. C/D/E oraz limity bez zmian.

Nie ustalono konkretnej pary powodującej stary konflikt: stary run jej nie zapisał; niezależna sonda processed600s/607775tx nie odtworzyła konfliktu w swoim ograniczonym oknie retencji. Nie przedstawiać hipotezy forka jako dowiedzionej przyczyny historycznej.

Walidacja finalnego kodu: parser135,Gate024,CPV31,CLI3,Python7 PASS; pięć regresji poprzedzone RED; fmt/diff-check/Clippy --no-deps/release PASS. Final release SHA256: `27c4638ccb4efc8972cac0e0ba3aaecbe3ca0d6fa8014e4120a366c727ff7302`.

Finalny run60s: `/root/gho_gate0_runs_20260927/gate0_repaired_confirmed_60s_20260930T162216Z.*` (JSONL,log,meta,audit.json,audit.md).38births/terminals,17snapshotów,zero naruszeń60pól/genericnulls/dropów/pendingexpiry,lag9ms,exit0/shutdownnull. Brak migracji i PhaseV w tej krótkiej próbce; poprawka migracji potwierdzona regresjami, nie nowym przypadkiem live.

Pośredni run360s na starszej binarce77e08ff…:224births,312snapshotów,2migracje+initial,CPV40clean/11insufficient/261source unavailable (każdy z niepełnym lookbackiem),drop0,lag32ms. Ten wynik **poprzedza** końcową poprawkę migracji i nie weryfikuje finalnej binarki.

ADR: `docs/ADR/ADR_8D_GATE0_OUTCOME_CONFLICT_PRICE_NULLS_20260930.md`. Logi/checkpoint/manifest: `/tmp/gate0_failure_repair_20260930/` oraz `.codex/active-task.md`. Self-review zakończony. Żaden Gate0 nie pozostał uruchomiony. Następnym niepotwierdzonym zakresem jest realna migracja i pełny600s survivor na finalnej binarce; nie wymuszać ich przez zmianę C/D/E.

GIT DELIVERY STATUS: COMMITTED — nic z Gate0; PUSHED — nic; PR — nie; LOCAL ONLY — wszystkie zmiany. NON-GIT LOCAL CONFIG: `/root/Gho/.env` endpoint `grpc.nln.clr3.org:443`, bez zmian w tej naprawie.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
GO-D nie było wejściem tej naprawy.

---

Poniżej zachowany wcześniejszy handoff i historia (stan historyczny):

# BIEŻĄCE POLECENIE — GEM 320 SOL / 1800 S W TLE

Na bezpośrednie polecenie użytkownika próg minimum MC dla etykiety Gem zmieniono z 350 na **320 SOL**. C/D/E i snapshot Phase V po600s pozostają bez zmian. Poprzednie progi/wyniki w sekcjach poniżej są historyczne.

Testy Gate0 18/18, formatter, diff-check i release build PASS. Zlecono start **1800 s** na nowej binarce (SHA256 `ba6cdd383dc445246825fcda03fb373644cb965ddd750f53b2b8ea7e25feff83`); faktyczny PID i start potwierdza plik meta runu. Artefakty będą miały prefiks `/root/gho_gate0_runs_20260927/gate0_320sol_1800s_`. Metadane zapisują SHA256 binarki/źródeł/configu, próg i PID. **Po potwierdzeniu startu zostawić w tle: bez pollingu, audytu, odczytów logów, restartu ani raportowania do powrotu użytkownika.** Wyniku tego runu nie deklarować przed nową instrukcją użytkownika.

ADR: `docs/ADR/ADR_8D_GATE0_GEM_320_SOL_BACKGROUND_1800S_20260930.md`.

---

# AKTUALIZACJA PO WZNOWIENIU — 2026-09-30, 14:02 UTC

Ta sekcja ma pierwszeństwo przed historycznym handoffem poniżej.

**GATE 0: NOT READY do 10 h.** Poprawki CPV/dev są w aktualnej binarce, lecz ostatni smoke 900 s zakończył się `lifecycle smoke has no phase-V snapshot` (exit 1). Brak Phase V nie jest wynikiem ekonomicznym.

- Zastane poprawki: zegar konsumenta CPV, opt-in protocol creator PDA, birth/snapshot provenance. Zweryfikowano je i zbudowano release.
- Nowa poprawka `gate0/metrics.rs`: nieznany developer daje DBIA `input_unavailable`, nie fałszywe `DBIA_NO_DEV_BUY`. Nowe testy w `gate0/tests.rs` obejmują ten przypadek i adapter progress/epoch/gap → indeks sesji.
- Pierwszy nowy smoke `smoke_cpvdev_final_900s_20260930T133440Z` zatrzymał się po ~560 s: jeden token miał 1231 successful swaps +15153 failed signatures, czyli limit 16384 w wieku ~190 s. Zmieniono **tylko profil** `configs/gate0.toml`: max_events_per_token=65536; global=131072 bez zmian; default kodu=16384 bez zmian. Fail-closed zachowany.
- Drugi smoke `smoke_cpvdev_final_900s_20260930T134739Z`: 467 births, 728 snapshotów, fazy **[450,148,129,1,0]**, 7 migracji i 7 initial states, 0 completed. Pełne 900 s pracy, ale exit 1 przez brak Phase V. Zero dropów/expiry, lag maks. 38 ms, shutdown_error=null.
- Odbiór 60 pól i identity: 728/728 zgodnych snapshotów; 467/467 births ze znanym creator. failed_tx_ratio, dev_buy_sol, dev_tx_ratio, dev_volume_ratio liczbowe 728/728. 15 snapshotów PumpSwap.
- CPV: 358 clean, 137 insufficient sample, 233 unavailable source; stały początek ciągłości we wszystkich snapshotach. Phase IV PumpSwap ma CPV clean i znaną identity. Phase V pozostaje niezweryfikowana na tej binarce.
- Większy budżet nie został obciążony powyżej starej granicy w drugim runie: największy terminal ma 5855 wpisów. Nie deklarować live dowodu pojemności >16384.
- Walidacja offline PASS: Gate0 18, CPV 30, parser 134, CLI 3, osiem dokładnych regresji FSC/DES/dev/top3, failed-filter 1, Python 6; fmt/check/diff i Clippy --no-deps PASS. Oba preflighty 60 s PASS.
- Binarka: `/root/gho_gate0_release_target/release/ghost_gate0`, SHA256 `17bdbb3dd03f39288a01d204c494b854fa744c94490f226e3ca59cda21d2f297`.
- Endpoint nadal `grpc.nln.clr3.org:443`. `/root/Gho/.env` nie zmieniano w tym wznowieniu.
- **COMMITTED: nic; PUSHED: nic; PR: nie; wszystkie zmiany LOCAL ONLY.** Run 10 h nie został uruchomiony. Po testach brak aktywnego Gate0; trenchd nietknięty.

Raport wszystkich 60 pól: `/root/gho_gate0_runs_20260927/smoke_cpvdev_final_900s_20260930T134739Z.audit.md` (+ `.audit.json`, `.meta.json`, `.jsonl`, `.log`). ADR: `docs/ADR/ADR_8D_GATE0_CPV_DEV_CONTINUITY_20260930.md`. Checkpoint: `.codex/active-task.md`. Logi offline: `/tmp/gate0_resume_validation/`, `/tmp/gate0_resume_final_tests.log`, `/tmp/gate0_resume_final_release.log`.

**Następny brakujący dowód:** pełny survivor do 600 s na bieżącej binarce/profilu, Phase V, completed z migration initial state, odbiór wszystkich conditional nulls i CPV/dev po migracji. Nie obniżać C/D/E ani nie traktować tego smoke jako PASS. Dla historycznego mintu z poprzedniego handoffu dev_wallet_known było false już w Phase I; nie udowodniono utraty identity na migracji ani konkretnego źródłowego creator tego mintu.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
GO-D nie był wejściem tego smoke; GO-E nie wykonywano.

---

# RAPORT ZDAWCZO-ODBIORCZY — GHOST / GATE 0 / METRIC-SURFACE / LIVE NLN

**Stan przekazywany:** 2026-09-30
**Cel dokumentu:** przejęcie prac przez innego agenta mającego dostęp do NeuGhost/VPS, repozytorium i narzędzi zdalnych.
**Zakres:** wyłącznie prace związane z Ghost / Gate 0, live ingestem NLN, pięcioma snapshotami, kompletną powierzchnią metryk, PumpSwap continuity, `failed_tx_ratio`, telemetryką kolejek, lifecycle smoke i gotowością do runu 10 h.

> **WAŻNE:** Nie resetować, nie czyścić i nie nadpisywać working tree. Zmiany są lokalne i niezacommitowane. Najpierw wykonać inspekcję `git status`, `git diff`, procesów i artefaktów. Nie robić commit/push/PR bez osobnej zgody użytkownika.

---

## 1. EXECUTIVE SUMMARY

Gate 0 jest **read-only obserwatorem badawczym**, nie trading runtime. Ma zebrać populację nowych tokenów Pump.fun, przeprowadzić je przez proste bramki C/D/E, zrobić do pięciu kumulatywnych snapshotów w wieku 30/90/180/300/600 s, zachować ciągłość po migracji do canonical PumpSwap, nadać terminalową etykietę Gem / non-Gem / unknown i dostarczyć dataset do późniejszej analizy separacji metryk.

Nie uruchamia OracleRuntime, Triggera, portfela ani egzekucji.

**Aktualny stan procesu:**
- transport / lifecycle / kolejki: **GREEN**,
- `failed_tx_ratio`: **GREEN, działa na realnych failed+successful transakcjach**,
- PumpSwap continuity i zweryfikowana cena post-trade: **GREEN w testach i live**,
- 60-polowa schema snapshotu: **GREEN — wszystkie 60 kluczy są obecne i mają field-status**,
- pełny 900 s lifecycle smoke na działającym endpointcie: **GREEN procesowo**,
- pozostały blocker przed 10 h: **CPV live rolling-state**; dodatkowo należy rozstrzygnąć, czy brak dev-identity w jedynym Phase V jest rzeczywistym bugiem continuity, czy poprawnym brakiem zweryfikowanego developera od samego birth.

**Nie uruchamiać jeszcze 10 h**, dopóki:
1. CPV w live Phase V nie ma poprawnego source/rolling-state, albo nie zostanie jednoznacznie udowodnione, że dla danego snapshotu `null` jest poprawnym statusem warunkowym;
2. dev identity nie zostanie prześledzone birth → session → migration → Phase V i rozstrzygnięte: bug continuity vs. prawidłowe `input_unavailable`;
3. po poprawkach nie przejdzie kolejny 900 s lifecycle smoke z odbiorem wartości + jakości wszystkich 60 pól.

---

## 2. LOKALIZACJE / WORKTREE / BRANCH / BUILD

### Główny worktree Gate 0
```text
/root/gho_gate0_20260927
```

Branch:
```text
agent/gate0-600s-20260927
```

Ostatni znany HEAD / baza:
```text
38acb83f492dc55b9244f0461c2d6b9e513771e4
```

Zmiany Gate 0 są **lokalne i niezacommitowane**. Nie było commit/push/PR/merge.

### Repo/operator env
```text
/root/Gho
/root/Gho/.env
```

Nie wypisywać wartości tokenów, API keys, walletów ani innych sekretów.

### Build targets
```text
/root/Gho_ingest/target
/root/gho_gate0_release_target
```

Aktualna release binarka Gate 0:
```text
/root/gho_gate0_release_target/release/ghost_gate0
```

Ostatni potwierdzony SHA256 aktualnej binarki Gate0 profile:
```text
6f108d366f557660096e2f9d0f974539c1f61dbeb9d1844b2340cd9cd8bdd3db
```

Binarka została po A/B probe przywrócona z kopii odpowiadającej właściwemu `with_gate0_observation()`.

### Katalog artefaktów live runów
```text
/root/gho_gate0_runs_20260927
```

---

## 3. CO DOKŁADNIE MA ROBIĆ GATE 0

### 3.1. Populacja i czas
- admission: **10 h**,
- obserwacja pojedynczego tokena: do **600 s**,
- drain po zamknięciu admission: do 600 s dla ostatnich tokenów.

### 3.2. Snapshoty
Dokładne wieku:
```text
30_000 ms
90_000 ms
180_000 ms
300_000 ms
600_000 ms
```

Snapshot musi być wykonany przed eviction i zawierać:
- wszystkie 60 wymaganych nazw metryk,
- dodatkowe skalary producentów,
- `quality.fields` dla każdego z 60 wymaganych pól,
- jawne przyczyny `null`,
- venue (`pump_curve` / `pumpswap`),
- provenance/quality producentów.

### 3.3. Bramki C/D/E

**C**
- odrzut, jeśli `<20` unikalnych successful transactions w pierwszych 30 s.

**D**
- odrzut, jeśli `<=100` unikalnych successful transactions do 180 s.

**E**
- od 180 s,
- rolling 30 s,
- co najmniej `3 successful swaps/s`,
- czyli co najmniej `90 successful swaps / 30 s`.

Ważne:
- C/D liczą **unikalne successful transaction signatures**,
- E liczy successful swap events,
- wiele CPI z jednego signature nie może pompować C/D.

### 3.4. Etykieta Gem
Gem tylko jeśli:
- przeżył C/D/E,
- faktycznie migrował do canonical PumpSwap,
- migracja nastąpiła >3 s po creation,
- migration initial state jest znany,
- post-migration observed market cap minimum do 600 s wynosi **>=350 SOL**.

Brak wymaganych stanów => label `null`, nie zgadywać.

### 3.5. Cel badawczy
Gate 0 ma sprawdzić, czy w populacji istnieje sygnał separujący Gem od kontroli zanim wchodzimy w formalną walidację / holdout / Gate 1.

Nie:
- ML w runtime,
- scoring autonomiczny,
- execution,
- evidence → execution,
- social/RPC backfill.

---

## 4. ARCHITEKTURA STANDALONE GATE 0

Binarka:
```text
ghost-launcher/src/bin/ghost_gate0.rs
```

Ścieżka logiczna:
```text
Yellowstone/Seer
→ IPC
→ SessionManager
→ TxIntelligence / fingerprint / sybil / CPV / FSC / AMM observation
→ Gate0 snapshots
→ append-only JSONL
```

Celowo brak:
```text
OracleRuntime
Trigger
wallet signing
execution
autonomous selection
```

To ma znaczenie dla logów: `ghost_gate0` nie uruchamia standardowego `main.rs::init_logging()` ani Oracle decision loggera. Dlatego jego własne artefakty to przede wszystkim JSONL + przekierowany stderr/stdout log, a nie zwykłe `system.log` i `oracle.log`.


---

## 5. 60 WYMAGANYCH METRYK

1. `tx_count`
2. `buy_count`
3. `sell_count`
4. `total_volume_sol`
5. `avg_tx_sol`
6. `volume_cv`
7. `volume_gini`
8. `top3_volume_pct`
9. `sell_buy_ratio`
10. `buy_ratio`
11. `sol_buy_ratio`
12. `fixed_size_buy_ratio`
13. `fixed_size_buy_ratio_1e4`
14. `failed_tx_ratio`
15. `avg_interval_ms`
16. `interval_cv`
17. `burst_ratio`
18. `timing_entropy`
19. `same_ms_tx_ratio`
20. `consecutive_buys`
21. `early_slot_volume_dominance_buy`
22. `early_top3_buy_volume_pct_3s`
23. `unique_signers`
24. `unique_ratio`
25. `hhi`
26. `tx_per_signer`
27. `successful_buy_signers`
28. `signer_cross_pool_velocity`
29. `cpv_other_pool_activity`
30. `fee_topology_diversity_index`
31. `fee_topology_diversity_index_v2`
32. `dev_buyer_infrastructure_affinity`
33. `spend_fraction_divergence`
34. `funding_source_concentration`
35. `market_cap_sol`
36. `bonding_progress_pct`
37. `price_change_ratio`
38. `single_tx_price_impact_pct`
39. `single_sell_impact_pct`
40. `single_sell_impact_pct_observed`
41. `dev_buy_sol`
42. `dev_tx_ratio`
43. `dev_volume_ratio`
44. `dev_paperhand_latency_ms`
45. `jito_tip_intensity`
46. `delta_jito_tip_intensity_1s_to_30s`
47. `delta_jito_tip_intensity_31s_to_300s`
48. `compute_unit_cluster_dominance`
49. `static_fee_profile_ratio`
50. `avg_inner_ix_count_50tx`
51. `avg_cpi_depth_50tx`
52. `flipper_presence_ratio`
53. `whale_reversal_ratio_top1`
54. `whale_reversal_ratio_top3`
55. `demand_elasticity_score`
56. `demand_elasticity_score_v2`
57. `momentum`
58. `demand`
59. `alpha_joint`
60. `alpha_sample`

### Zasada odbioru
Nie wymuszać sztucznie liczby tam, gdzie metryka jest:
- not applicable,
- insufficient sample,
- input unavailable,
- degraded/unknown.

Poprawny snapshot ma:
- wszystkie 60 kluczy,
- dla każdego `quality.fields[name].status`,
- jeśli wartość jest `null`, co najmniej jeden jawny powód.

**Nie clampować, nie wstawiać zera „dla kompletności”.**

---

## 6. NAJWAŻNIEJSZE PLIKI DODANE / ZMIENIONE

### Nowe / Gate0
```text
ghost-launcher/src/gate0.rs
ghost-launcher/src/gate0/metrics.rs
ghost-launcher/src/gate0/tests.rs
ghost-launcher/src/bin/ghost_gate0.rs
off-chain/components/seer/src/amm_observation.rs
configs/gate0.toml
docs/GATE0.md
scripts/gate0_scan.py
scripts/test_gate0_scan.py
.agent/PLAN.md
.agent/STATE.md
.agent/LOG.md
```

### Istotne zmienione
```text
ghost-core/src/tx_intelligence/types.rs
ghost-launcher/src/session/observation.rs
ghost-launcher/src/tx_intelligence/engine.rs
ghost-launcher/src/tx_intelligence/funding_source.rs
ghost-launcher/src/tx_intelligence/sybil_metrics.rs
ghost-launcher/src/metric_contracts/pr2b.rs
ghost-launcher/src/components/seer.rs
ghost-launcher/src/session/manager.rs
off-chain/components/seer/src/grpc_connection.rs
off-chain/components/seer/src/lib.rs
off-chain/components/seer/src/binary_parser.rs
off-chain/components/seer/src/ipc.rs
off-chain/components/seer/src/config.rs
off-chain/components/seer/src/types.rs
off-chain/components/seer/src/nln_program_streams.rs
```

W working tree były również zmodyfikowane liczne rollout configi z aktualizacją domen endpointów. **Nie robić `git clean`, `git reset --hard` ani checkout całego drzewa.**


---

## 7. KLUCZOWE ZMIANY IMPLEMENTACYJNE

### 7.1. Gate0 queue / throughput / root-cause telemetry
Dodano:
- ingress high-water,
- received,
- overflow dropped,
- IPC egress/downstream depth,
- max consumer lag,
- pending mapping buffered/expired,
- rozdzielenie root `reason` od `shutdown_error`.

Gate0 profile:
- Tokio workers: 8,
- ingress: 16384,
- IPC: 100000,
- produkcyjne defaulty zwykłego Seera pozostawione bez zmian.

Wynik: problem `ingress_queue_saturated` z pierwszych smoke został zamknięty.

### 7.2. Pending mapping / cohort boundary
Produkcja:
- TTL pending mapping pozostaje historyczne ~30 ms.

Gate0:
- TTL 5 s,
- explicit cohort:
  - `gate0_cohort_pools`,
  - `gate0_cohort_mints`,
  - `gate0_observation_mode`.

Pre-start stare pending events nie są traktowane jako coverage loss dla kohorty Gate0.
Po accepted birth `gate0_register_cohort(pool,mint)` oznacza już zbuforowane pasujące trades jako coverage-relevant.

Regresje:
- `gate0_preexisting_pending_expiry_is_diagnostic_not_coverage_loss`
- `gate0_birth_marks_buffered_trade_as_coverage_relevant`

### 7.3. Trade candidate false positives / wrappers
Naprawiono:
- generic Jupiter / DFlow wrapper nie jest sam w sobie dowodem Pump/PumpSwap trade,
- PumpSwap wrapper jest kandydatem tylko dla właściwych Buy/Sell event discriminatorów,
- known Pump mapping fallback pozostaje.

W live nadal bywają ostrzeżenia `GATE0_TRADE_CANDIDATE_PARSE_MISS` dla pewnych layoutów/route wrappers. Nie rozszerzać parsera w ciemno. Najpierw ustalić, czy dany miss jest faktycznie coverage-relevant dla kohorty Gate0.

### 7.4. top3 signer volume ratio
Naprawiono denominator:
- ma być suma signer volumes,
- nie zewnętrzny total.

Regresje:
- `top3_signer_ratio_uses_signer_volume_denominator_not_external_total`
- `top3_signer_ratio_is_exact_one_when_top3_is_the_full_population`

### 7.5. dev_volume_ratio floating drift
Stary błąd:
- numerator dev był budowany z per-signer stats,
- denominator z osobnego globalnego akumulatora,
- przy prawie 100% dev activity różne kolejności akumulacji f64 mogły dać minimalnie >1.

Nie zastosowano clampu.
Dodano denominator z tej samej populacji signerów.

Regresja:
- `dev_volume_denominator_is_built_from_the_same_signer_population`

### 7.6. FSC V2 overflow >255 buyerów
Wire V2 ma buyer counts jako `u8`.

Błąd:
- count saturacja do 255,
- coverage liczony z pełnego N,
- PR2A widział niespójność.

Fix:
- jawny bounded V2 sample do 255,
- full-population legacy/observation FSC zachowuje pełną populację,
- overflow oznaczany jako degraded reason, nie ciche fałszowanie.

Regresje:
- `fsc_v2_bounds_buyer_sample_to_wire_capacity_without_coverage_mismatch`
- `observation_fsc_full_population_is_not_reduced_to_u8_wire_sample`

Dodatkowo Gate0 ma `observation_history_ms`, żeby zachować historię potrzebną do ponownego policzenia FSC w późnych fazach, **bez rozszerzania semantycznego lookbacku pojedynczej BUY**.

Regresja:
- `observation_fsc_retains_old_buy_history_without_widening_attribution`

### 7.7. `failed_tx_ratio`

Problem pierwotny: PrimaryGlobal provider filter miał `failed = Some(false)`, więc failed tx nie docierały i `failed_tx_ratio` było zawsze `null`.

Fix: dodano osobny:
```text
GrpcSubscriptionProfile::Gate0Observation
```

który:
- nadal `vote = Some(false)`,
- PumpFun + PumpSwap account include,
- `failed = None`,
- zwykły `PrimaryGlobal` pozostaje `failed = Some(false)`.

W Gate0:
- successful signatures i failed signatures są rozdzielone,
- failed tx nie zwiększają successful swap/volume,
- ratio:
```text
unique_failed_transaction_signatures / unique_attempted_transaction_signatures
```

Regresje:
- `gate0_observation_includes_failed_without_changing_primary`
- `failed_attempt_ratio_is_transaction_deduplicated_and_never_counts_as_volume`

W ostatnim Phase V live:
```text
failed_tx_ratio = 0.7239188105373557
```

Źródło jakości:
```text
gate0_observation_success_and_failure
```

Ten blocker jest **zamknięty**.

### 7.8. PumpSwap verified market price / cross-venue DES
Dodano do `DesPriceSourceV2`:
```text
VerifiedMarketPostTradePrice
```

Gate0 observation może korzystać ze zweryfikowanego post-state price w wspólnej jednostce SOL/token.

Zmiany w `sybil_metrics.rs` obejmują observation-mode:
- first-buyer ordering bez zgadywania,
- możliwość użycia invariant inputu, jeśli kolejność jest nieznana, ale każdy możliwy pierwszy BUY daje identyczne wejście,
- cross-venue DES,
- cena `verified_market_price`,
- nie mieszamy niespójnych źródeł ceny.

Regresje:
- `observation_invariant_first_buy_does_not_guess_order_or_change_default`
- `observation_des_prices_cross_venue_but_unknown_order_splits_triples`

W finalnym Phase V:
- `demand_elasticity_score_v2` był liczbowy,
- `price_source = verified_market_post_trade_price`,
- candidate triples / closed triples były kompletne.

### 7.9. Phase V / 60-field surface
Test:
```text
phase_five_keeps_pumpswap_buyers_verified_prices_and_all_field_statuses
```

Wymaga:
- 5 faz,
- Phase V venue `pumpswap`,
- `quality.fields` ma dokładnie 60 wpisów,
- każde pole: number lub null,
- null => niepusta lista reasons,
- kluczowe metryki PumpSwap w fixture są finite:
  - FTDI v2,
  - DBIA,
  - CPV,
  - DES v2,
  - price impacts,
  - failed_tx_ratio.


---

## 8. HISTORIA SMOKE / NAJWAŻNIEJSZE INCYDENTY

### 8.1. Pierwsze smoke — queue saturation
Próby 2026-09-27:
- `ingress_queue_saturated` przy 2048,
- zero accepted births,
- nie były badawczym datasetem.

Po profilu 16384/100000 problem przepustowości zniknął.

### 8.2. Clean 120 s smoke po cohort/top3
Przykład:
```text
/root/gho_gate0_runs_20260927/smoke_top3fix_120s_20260928T213335Z.jsonl
```
Zero queue drops / pending expiry.

### 8.3. 900 s smoke — PR2B manipulation invariant
Run:
```text
smoke_lifecycle_900s_20260928T214731Z
```
Padł przez:
```text
PR2B producer invariant failed: manipulation numeric field range
```
Infrastruktura była zdrowa.
Dodano field-specific PR2B errors.

### 8.4. dev ratio fix
Po analizie producenta:
- `dev_volume_ratio` denominator poprawiony,
- testy GREEN.

### 8.5. FSC known coverage failure
Kolejny run ujawnił:
```text
fsc.v2_known_coverage
```

Root cause:
- u8 count saturation vs. full-N coverage.

Naprawione jak opisano wyżej.

### 8.6. Pierwszy pełny poprawny 900 s lifecycle przed metric-surface hardening
Artefakt:
```text
/root/gho_gate0_runs_20260927/smoke_finalfix_900s_20260929T002558Z.jsonl
/root/gho_gate0_runs_20260927/smoke_finalfix_900s_20260929T002558Z.log
```

Summary:
- admitted 333,
- phases `[316,125,108,4,1]`,
- migrations 10,
- migration initial states 10,
- completed_with_migration_initial_state 1,
- no drops,
- pending expired 0.

To udowodniło lifecycle, ale snapshot Phase V miał wtedy wiele missing metrics i `failed_tx_ratio=null`.

---

## 9. ENDPOINTY NLN — BARDZO WAŻNY STAN

### 9.1. Endpointy
Nowa domena gRPC:
```text
grpc.mainnet.solana.nolimitnodes.com:443
```

Legacy działająca domena:
```text
grpc.nln.clr3.org:443
```

Program Streams:
- code default bywa `streams.mainnet.solana.nolimitnodes.com:443`,
- aktywne rollout/env historycznie często `events.mainnet.solana.nolimitnodes.com:443`.

RPC:
- NLN public product route testowany: `rpc.mainnet.solana.nolimitnodes.com`
- zwykły Ghost trigger RPC w env wskazuje na SimplyStaking; nie zmieniać bez potrzeby.

### 9.2. Root cause martwych smoke po zmianie domeny

BARDZO ISTOTNE: provider ogólnie **nie był down**.

#### Bez auth, ręczny gRPC Ping przez curl HTTP/2 na nowym hostname
Backend odpowiadał natychmiast:
```text
HTTP/2 200
grpc-status: 16
grpc-message: missing auth credential
```

Czyli DNS/TCP/TLS/ALPN h2/backend ingress działały.

#### Z realnym key na nowym hostname
Authenticated gRPC:
- oficjalny Yellowstone SDK + `x-token`: Ping/GetSlot timeout,
- raw tonic + `x-api-key`: Ping/GetSlot timeout,
- ręczny curl framed gRPC + `x-api-key`: timeout bez response headers.

#### Ten sam key na NLN JSON-RPC
```text
getHealth = ok
getSlot = poprawny bieżący slot
```

Czyli key jest aktywny.

#### Ten sam key na legacy gRPC hostname
Ręczny framed Ping:
```text
grpc.nln.clr3.org
grpc-status: 0
Pong bytes: 00000000020801
```

Działa natychmiast.

#### DNS
Oba hostname’y rozwiązywały się do tego samego IP:
```text
64.130.40.90
```

Wniosek operacyjny:
**różnica to authenticated Host/SNI routing na nowym hostname dla tego node/key.**

### 9.3. `trenchd`
Działający `trenchd` nie został ruszony.

Jego config wskazywał:
```text
https://grpc.nln.clr3.org:443
x-api-key
NLN_API_KEY
```

`NLN_API_KEY` i `GHOST_SEER_GRPC_X_TOKEN` zostały sprawdzone jako **ta sama wartość**, bez jej wyświetlania.

Pełny Gate0 primary+funding działał jednocześnie z `trenchd`, więc legacy endpoint obsłużył ten układ.

### 9.4. Zmiana env
W `/root/Gho/.env` przywrócono:
```text
GHOST_SEER_GRPC_ENDPOINT=grpc.nln.clr3.org:443
```

**Przed kolejnym runem sprawdzić, czy nadal tak jest.**

Nie przełączać ponownie na `grpc.mainnet.solana.nolimitnodes.com` bez nowego testu authenticated Ping.


---

## 10. OSTATNI NAJWAŻNIEJSZY 900 s METRIC-SURFACE SMOKE

Artefakty:
```text
/root/gho_gate0_runs_20260927/smoke_metric_surface_legacy_900s_20260930T004445Z.jsonl
/root/gho_gate0_runs_20260927/smoke_metric_surface_legacy_900s_20260930T004445Z.log
```

Rozmiary:
- JSONL ~11 MB,
- log ~913 KB.

### 10.1. Proces / infrastruktura
```text
reason = smoke_only
shutdown_error = null
```

Summary:
```text
admitted = 558
phase_counts = [539, 135, 125, 1, 1]
migrations = 12
migration_initial_states = 12
completed_with_migration_initial_state = 1
gems = 0
non_gems = 516
label_unavailable = 42
```

Terminal counts:
```text
C = 392
D = 78
E = 45
completed = 1
smoke_only = 42
```

Pool:
```text
pool_events_seen = 692
non_wsol_quote rejections = 43
observation_success_not_true = 47
```

Runtime:
```text
events_consumed ≈ 1,081,803
primary received ≈ 752,274
funding received ≈ 1,086,541
primary overflow dropped = 0
funding overflow dropped = 0
pending expired = 0
max consumer lag = 140 ms
primary high-water = 1511 / 16384
funding high-water = 273 / 16384
IPC downstream sampled high-water = 108 / 100000
```

**Transport/lifecycle PASS.**

---

## 11. JEDYNY PHASE V Z OSTATNIEGO RUNU

Mint:
```text
tZb9aZdDRS95vVqoKgphYZQnGScFV1gdggctvsJPSob
```

Venue:
```text
pumpswap
```

Lifecycle:
```text
migration_age_ms = 275765
migration_initial_seen = true
last_phase = 5
post_migration_min_mc_sol = 334.20362081956165
gem = false
label_reasons = []
```

Nie był Gemem, bo:
```text
334.20 SOL < 350 SOL
```

To jest poprawna etykieta non-Gem.

---

## 12. PHASE V — KOMPLETNOŚĆ 60 PÓL

W Phase V:
- `quality.fields` ma **60/60 kluczy**,
- 50 wartości liczbowych,
- 10 nulli z jawnym statusem/powodem.

### 12.1. Null poprawny / warunkowy

#### `bonding_progress_pct`
```text
status = not_applicable
reason = pumpswap_has_no_bonding_curve
```
**Nie naprawiać.** Po canonical migration bonding progress nie ma sensu.

#### `delta_jito_tip_intensity_1s_to_30s`
```text
status = insufficient_sample
reason = jito_anchor_insufficient_observations
```
To może być poprawny conditional null.

#### `static_fee_profile_ratio`
```text
status = insufficient_sample
reason = STATIC_FEE_MIN_BUYS
```
To może być poprawny conditional null.

#### `dev_buyer_infrastructure_affinity`
```text
status = not_applicable
reason = DBIA_NO_DEV_BUY
```
Może być poprawny, **jeśli developer identity była znana i rzeczywiście nie było dev BUY**.
Trzeba rozstrzygnąć razem z identity continuity.

---

## 13. AKTUALNE BLOCKERY DO NAPRAWY / ROZSTRZYGNIĘCIA

### 13.1. CPV rolling state — najbardziej prawdopodobny realny bug live wiring

Phase V:
```text
signer_cross_pool_velocity = null
cpv_other_pool_activity = null
```

Status:
```text
CPV_ROLLING_STATE_UNAVAILABLE
input_unavailable
```

Quality:
```text
sample_count = 1391
required_clean_sample_count = 3
rolling_state_available = false
source = "unavailable"
quality = "unavailable_source"
```

To jest bardzo mocna wskazówka:
- signer sample jest ogromny,
- problemem nie jest liczba buyerów,
- nie ma source-progress / rolling coverage state.

#### Co sprawdzić
```bash
rg -n "CPV_ROLLING_STATE_UNAVAILABLE|rolling_state_available|observe_source_progress|cross_pool_velocity"   ghost-launcher/src
```

W fixture Phase V test ręcznie robi:
- `cross_pool_velocity_index()`,
- `observe_source_progress(...)`,
- `observe_buy(...)` dla innego marketu.

W live prawdopodobnie `PrimaryTradeFeedProgress` dochodzi do Gate0, ale nie jest podawany do CPV index / SessionManager w sposób, którego oczekuje producent.

**Nie sztucznie ustawiaj `rolling_state_available=true`.**
Trzeba podłączyć realny source progress / gap state z tego samego primary feed.

#### Oczekiwany fix
Najbardziej prawdopodobny:
- obsługa `SeerEvent::PrimaryTradeFeedProgress` w Gate0 / istniejącym adapterze sesji,
- przekazanie epoch/watermark/gap do tego samego CPV index, którego używa materializer,
- po gap/reconnect CPV ma fail-closed dopóki rolling continuity nie zostanie odzyskane.

Nie wprowadzać nowego źródła czasu ani backfillu.

### 13.2. Developer identity — najpierw ustalić, czy to bug continuity

Phase V null:
```text
dev_buy_sol
dev_tx_ratio
dev_volume_ratio
dev_paperhand_latency_ms
```

Reason:
```text
verified_developer_identity_unavailable
```

DBIA:
```text
DBIA_NO_DEV_BUY
```

#### UWAGA
Nie ma jeszcze dowodu, że identity była znana na birth i została zgubiona.

Należy najpierw sprawdzić dla dokładnie tego mintu:
- birth row,
- PoolDetected candidate creator/dev evidence,
- session `dev_wallet`,
- czy identity była kiedykolwiek verified,
- czy zniknęła dopiero po migration,
- czy PumpSwap tx route zmienił session/pool mapping.

#### Jeśli identity była znana na birth
Wtedy fix:
- zachować verified dev wallet jako token/session identity przez migrację,
- nie re-derive po PumpSwap,
- canonical PumpSwap pool musi należeć do tej samej logical token/session identity.

#### Jeśli identity NIE była zweryfikowana od birth
Wtedy:
- `verified_developer_identity_unavailable` jest prawidłowym `null`,
- nie wolno zgadywać creator/dev z niepewnego źródła,
- nie wymuszać wartości tylko dla 60/60 numeric.

Pomocnicze search:
```bash
rg -n "dev_wallet|verified_developer_identity|is_dev_buy|creator"   ghost-launcher/src/session   ghost-launcher/src/gate0*   ghost-launcher/src/tx_intelligence   off-chain/components/seer/src
```


---

## 14. INNE OBSERWOWANE QUALITY / NIE SĄ AKTUALNIE GŁÓWNYM BLOCKEREM

### FSC w Phase V
FSC działa, ale coverage jest ograniczone i V2 degraded.

Przykładowo:
- buyer sample ~1409,
- known source count ~197,
- wiele unknown z powodu:
  - no retained recipient history,
  - no prebuy transfer in window,
  - per-recipient history overflow,
  - same-slot ordering unavailable,
  - low attribution confidence.

FSC V2:
- total buyers wire = 255,
- known buyers = 20,
- known coverage ~0.0784,
- status degraded,
- reason m.in. same-slot ordering unavailable,
- `FSC_V2_BUYER_SAMPLE_TRUNCATED`.

To jest jawnie reprezentowane; nie maskować.

### FTDI V2
W Phase V:
- liczbowy,
- represented signer count 1391,
- unique topology count 11,
- FTDI v2 ~0.7639.

### DES V2
W Phase V:
- candidate triples = 2448,
- closed triples = 2448,
- priced buys = 2450,
- price source = `verified_market_post_trade_price`,
- DES v2 ~ -0.05447.

To potwierdza, że PumpSwap price continuity działa.

---

## 15. TESTY, KTÓRE BYŁY GREEN

Precyzyjne testy, które warto ponownie uruchomić po zmianach:

```text
gate0_observation_includes_failed_without_changing_primary
failed_attempt_ratio_is_transaction_deduplicated_and_never_counts_as_volume
phase_five_keeps_pumpswap_buyers_verified_prices_and_all_field_statuses
observation_fsc_retains_old_buy_history_without_widening_attribution
observation_fsc_full_population_is_not_reduced_to_u8_wire_sample
observation_invariant_first_buy_does_not_guess_order_or_change_default
observation_des_prices_cross_venue_but_unknown_order_splits_triples
dev_volume_denominator_is_built_from_the_same_signer_population
top3_signer_ratio_uses_signer_volume_denominator_not_external_total
top3_signer_ratio_is_exact_one_when_top3_is_the_full_population
fsc_v2_bounds_buyer_sample_to_wire_capacity_without_coverage_mismatch
```

### Uwaga o szerokim filtrze testów
Jedna komenda z broad pattern:
```text
cargo test ... observation_ tx_intelligence
```

złapała unrelated `oracle_runtime` test:
```text
pool_observation_task_wires_pr5_checkpoint_and_materialization
```

i dostała stack overflow.

Nie traktowano tego jako regresji Gate0.
Po tym odpalano testy po dokładnych nazwach i były GREEN.

---

## 16. WIDE REPO BASELINE / CLIPPY

Wcześniej szeroki Clippy z dependencies zatrzymywał się na istniejącym, niezmienionym:
```text
ghost-brain/src/pipeline/execution.rs
clippy::never_loop
```

Clippy `--no-deps` dla zmienionych pakietów przechodził.

Nie mieszać baseline warnings z regresją Gate0.

---

## 17. LOGOWANIE I ROZMIAR DANYCH

`ghost_gate0` nie uruchamia zwykłego:
- `system.log`,
- `oracle.log/oracle_decision.log`.

Ma własny tracing do stderr i append-only Gate0 JSONL.

Ostatni 900 s run:
- JSONL ~11 MB,
- log ~0.9 MB.

Proste liniowe 10 h dla obecnego slim Gate0:
- ~40× ten footprint,
- rząd ~0.45–0.50 GB,
- zależny od market activity.

To **nie** jest rozmiar pełnego zwykłego Ghost runtime z Oracle/system logs.

---

## 18. STAN ENDPOINTÓW / AUTH — PRAKTYCZNE ZASADY

### Gate0 / Yellowstone gRPC teraz
Używać:
```text
grpc.nln.clr3.org:443
```

Auth dla raw tonic path:
```text
x-api-key
```

Key env:
```text
GHOST_SEER_GRPC_X_TOKEN
```

Ten sam key jest też w `NLN_API_KEY`.

### Nie używać obecnie
```text
grpc.mainnet.solana.nolimitnodes.com:443
```
bez ponownego authenticated Ping.

### Program Streams
Stan był niejednorodny:
- `streams.mainnet.solana.nolimitnodes.com:443` — code default,
- `events.mainnet.solana.nolimitnodes.com:443` — wiele rolloutów / env.

W ręcznym `ListTopics`:
- `events.mainnet...` odpowiedział `grpc-status: 0`,
- `streams.mainnet...` odpowiedział `RESOURCE_EXHAUSTED: concurrent stream limit reached`.

Gate0 sam ma `program_streams.enabled = false`, więc to nie jest obecny primary ingest path.


---

## 19. CO ZROBIĆ TERAZ — KOLEJNOŚĆ

### Krok 0 — nie niszcz working tree
```bash
cd /root/gho_gate0_20260927

git status --short --branch
git diff --check
git diff --stat
git diff -- ghost-launcher/src/gate0.rs            ghost-launcher/src/gate0/metrics.rs            ghost-launcher/src/gate0/tests.rs            ghost-launcher/src/session/observation.rs            ghost-launcher/src/tx_intelligence/sybil_metrics.rs            ghost-launcher/src/tx_intelligence/funding_source.rs            off-chain/components/seer/src/grpc_connection.rs            off-chain/components/seer/src/lib.rs            off-chain/components/seer/src/amm_observation.rs            off-chain/components/seer/src/binary_parser.rs
```

### Krok 1 — potwierdź endpoint
Bez drukowania tokenu:
```bash
grep '^GHOST_SEER_GRPC_ENDPOINT=' /root/Gho/.env
```

Oczekiwane:
```text
GHOST_SEER_GRPC_ENDPOINT=grpc.nln.clr3.org:443
```

### Krok 2 — potwierdź binarkę
```bash
sha256sum /root/gho_gate0_release_target/release/ghost_gate0
```

Ostatni znany:
```text
6f108d366f557660096e2f9d0f974539c1f61dbeb9d1844b2340cd9cd8bdd3db
```

Jeśli hash inny — sprawdź, co zbudowano od czasu raportu.

### Krok 3 — przeczytaj Phase V z finalnego smoke
Artefakt:
```text
/root/gho_gate0_runs_20260927/smoke_metric_surface_legacy_900s_20260930T004445Z.jsonl
```

Sprawdź:
- mint,
- birth,
- wszystkie phase rows,
- terminal,
- `quality.fields`,
- CPV quality,
- dev identity evidence.

### Krok 4 — napraw CPV source progress
Nie dotykaj definicji metryki.
Napraw live wiring source-progress / gap continuity.

Po fixie dopisz regresję:
- Gate0 live-style source progress dociera do tego samego CPV index,
- reconnect/gap → unavailable,
- fresh continuous progress → available,
- canonical PumpSwap nie jest innym marketem dla tego samego tokena.

### Krok 5 — rozstrzygnij dev identity
Najpierw dowód:
- known at birth?
- known in session?
- lost after migration?
- never known?

Tylko jeśli faktycznie lost-after-migration — fix continuity.

Dodaj test:
- verified creator/dev at birth,
- token migrates,
- Phase V dev metrics nadal widzą tę samą verified identity.

Nie tworzyć identity, jeśli source evidence nie istnieje.

### Krok 6 — pełne testy targeted
Minimum:
```bash
cargo fmt --all -- --check
git diff --check

cargo test --locked --offline -p seer --lib gate0_observation_includes_failed_without_changing_primary
cargo test --locked --offline -p ghost-launcher --lib failed_attempt_ratio_is_transaction_deduplicated_and_never_counts_as_volume
cargo test --locked --offline -p ghost-launcher --lib phase_five_keeps_pumpswap_buyers_verified_prices_and_all_field_statuses
cargo test --locked --offline -p ghost-launcher --lib observation_fsc_retains_old_buy_history_without_widening_attribution
cargo test --locked --offline -p ghost-launcher --lib observation_fsc_full_population_is_not_reduced_to_u8_wire_sample
cargo test --locked --offline -p ghost-launcher --lib observation_invariant_first_buy_does_not_guess_order_or_change_default
cargo test --locked --offline -p ghost-launcher --lib observation_des_prices_cross_venue_but_unknown_order_splits_triples
cargo check --locked --offline -p ghost-launcher --bin ghost_gate0
```

Dodać nowe dokładne testy CPV/dev continuity.

### Krok 7 — release build
```bash
export CARGO_TARGET_DIR=/root/gho_gate0_release_target
export CARGO_BUILD_JOBS=4
export CARGO_INCREMENTAL=0

cargo build --locked --offline --release -p ghost-launcher --bin ghost_gate0
sha256sum /root/gho_gate0_release_target/release/ghost_gate0
```

### Krok 8 — 60 s preflight
Na legacy endpoint, z `x-api-key`.

Warunki:
- eventy >0,
- primary/funding received >0,
- births >0,
- drops = 0,
- pending expiry = 0,
- brak `primary progress unavailable/stale`.

### Krok 9 — 900 s lifecycle smoke
Odbiór musi obejmować:
- process `reason=smoke_only`,
- shutdown_error null,
- Phase V >0,
- completed >0,
- completed_with_migration_initial_state >0,
- zero ingress drops,
- zero coverage expiry,
- 60/60 `quality.fields`,
- lista każdego nulla + reason/status,
- `failed_tx_ratio` liczbowy,
- CPV dostępny tam, gdzie source continuity jest clean,
- dev identity zachowana, jeśli była zweryfikowana na birth.

### Krok 10 — dopiero wtedy GO na 10 h
Nie wystarczy „proces przeżył”.
Potrzebny jest:
- PASS procesu,
- PASS kompletności schema,
- PASS poprawności conditional nulls,
- PASS PumpSwap continuity,
- PASS failed-tx source,
- brak realnych source-wiring dziur.

---

## 20. ACCEPTANCE CRITERIA DLA KOLEJNEGO SMOKE

### PASS infrastruktury
- `overflow_dropped == 0` primary/funding,
- `pending_mapping.expired_total == 0`,
- brak source gap powodującego przerwanie,
- Phase V istnieje,
- completed lifecycle istnieje.

### PASS 60-field surface
Dla każdego phase:
- dokładnie 60 field statuses,
- każdy required metric key istnieje,
- number albo null,
- null ma jawny reason.

### Null akceptowalny
Przykładowo:
- bonding progress po PumpSwap,
- brak dev buy,
- insufficient sample dla krótkiego early-window,
- brak wymaganej liczby Jito observations.

### Null nieakceptowalny
Jeżeli:
- input powinien być dostępny z live source,
- sample_count jest duży,
- brak wynika z niewpiętego source progress,
- identity była zweryfikowana wcześniej, ale została zgubiona po migracji.


---

## 21. RZECZY, KTÓRYCH NIE ROBIĆ

1. Nie resetować working tree.
2. Nie robić `git clean -fd`.
3. Nie commitować/pushować bez zgody użytkownika.
4. Nie mieszać zmian z Armed Gunman / trenchd.
5. Nie zatrzymywać działającego `trenchd`.
6. Nie drukować `/root/Gho/.env`.
7. Nie wypisywać tokenów/API keys/wallet secrets.
8. Nie przywracać nowego NLN gRPC hostname bez authenticated Ping.
9. Nie wstawiać zer zamiast nulli.
10. Nie clampować metryk tylko po to, żeby przejść kontrakt.
11. Nie rozszerzać parsera wrapperów bez dowodu, że event jest rzeczywiście Pump/PumpSwap trade.
12. Nie uruchamiać 10 h przed kolejnym poprawnym 900 s metric-surface smoke.

---

## 22. DODATKOWE UWAGI O OBECNYM WORKTREE

Ostatni znany `git status` zawierał dużo zmodyfikowanych tracked files i kilka untracked Gate0 files. Poza plikami wyżej były również rollout configi i wcześniejsze zmiany infrastrukturalne.

To jest powód, żeby przed jakąkolwiek edycją:
- zrobić `git status`,
- `git diff --stat`,
- przejrzeć diff dokładnie w plikach CPV/dev,
- nie zakładać, że repo jest czyste.

---

## 23. STAN NARZĘDZIOWY PRZY PRZEKAZANIU

Remote Desktop Commander osiągnął miesięczny limit i kolejne wywołania zostały zablokowane. NeuGhost był nadal sparowany/połączony; problemem nie był VPS.

To jest bezpośredni powód przekazania prac innemu agentowi z dostępem do VPS/tools.

Nie było żadnych nowych zmian po tym momencie.

---

## 24. NAJWAŻNIEJSZY WNIOSEK DLA PRZEJMUJĄCEGO AGENTA

Nie wracaj do starych problemów kolejek, `failed_tx_ratio`, top3, dev ratio, FSC u8 ani PumpSwap price continuity — te zostały już zdiagnozowane i naprawione.

Aktualna robota jest znacznie węższa:

1. **CPV rolling-state live wiring**
   - sample istnieje,
   - source continuity nie dochodzi do producenta.

2. **Developer identity**
   - najpierw ustalić, czy to prawdziwa utrata continuity,
   - dopiero potem fix.

3. **Regresje**
   - żadnych nowych problemów w zwykłym runtime,
   - Gate0 observation ma być opt-in.

4. **Kolejny 900 s smoke**
   - legacy `grpc.nln.clr3.org`,
   - pełny primary + funding,
   - odbiór 60 pól i wszystkich conditional nulls.

5. **Dopiero potem 10 h.**

---

## 25. SZYBKI START — CHECKLISTA 5 MINUT

```bash
cd /root/gho_gate0_20260927

git status --short --branch
git diff --check

grep '^GHOST_SEER_GRPC_ENDPOINT=' /root/Gho/.env
sha256sum /root/gho_gate0_release_target/release/ghost_gate0

pgrep -a ghost_gate0 || true
pgrep -a trenchd || true

ls -lh /root/gho_gate0_runs_20260927/smoke_metric_surface_legacy_900s_20260930T004445Z.*

rg -n "CPV_ROLLING_STATE_UNAVAILABLE|rolling_state_available|observe_source_progress|cross_pool_velocity" ghost-launcher/src
rg -n "verified_developer_identity|dev_wallet|is_dev_buy|creator"   ghost-launcher/src/session   ghost-launcher/src/gate0*   ghost-launcher/src/tx_intelligence   off-chain/components/seer/src
```

Następnie:
- odtworzyć Phase V mint `tZb9aZdDRS95vVqoKgphYZQnGScFV1gdggctvsJPSob`,
- ustalić CPV source progress,
- prześledzić dev identity,
- minimalny fix,
- targeted tests,
- release,
- 60 s preflight,
- 900 s lifecycle smoke,
- dopiero GO/NO-GO dla 10 h.

---

**Koniec raportu.**
