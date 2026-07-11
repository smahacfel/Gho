# ADR-8D: Korekta kontraktów interpretacyjnych dziesięciu metryk Ghost

Status: AUDIT_COMPLETE / REPORT_CORRECTED / NO_RUNTIME_CHANGE
Typ: ADR-8D / metric interpretation audit
Data: 2026-07-10
Repo: `/root/Gho_dynamic_exit_v1`
HEAD podczas pracy: `f3318f3`
Zakres: dokładnie 10 metryk z listy przekazanej przez użytkownika
Poziom ryzyka: LOW runtime risk / MEDIUM analytical and rollout risk

Uwaga o szablonie: wymagana przez instrukcje ścieżka
`docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Dokument zachowuje
lokalny, sekcyjny format ADR-8D używany w repo i jawnie zapisuje decyzję,
evidence, konsekwencje, weryfikację oraz kontrolę zakresu.

## 1. Problem

Dziesięć istniejących nazw lub statusów metryk pozwalało na interpretacje
szersze niż faktyczny kontrakt producenta. Ryzyko nie ograniczało się do
dokumentacji: część nazw jest współdzielona przez różne źródła, populacje,
denominatory albo warstwy active/compat/shadow/export-only.

Dotychczasowa wersja raportu również zawierała istotne uproszczenia:

- traktowała `dev_volume_ratio` jako total exposure, choć jest udziałem gross
  dev turnover,
- nie odróżniała canonical MFS od mocniejszej semantyki GatekeeperBuffer compat
  dla pierwszego zakupu deva,
- nie opisywała, że FTDI dla dwóch próbek może być `Some(value)` z degraded
  evidence,
- upraszczała `flip_ratio_10s` do samego limitu slotów,
- nie ograniczała wpływu `evidence_status.fsc` do profilu V3, który rzeczywiście
  wymaga FSC,
- wychodziła poza przekazany zakres przez jedenasty finding NLN.

## 2. Decyzja

Poprawiono i przyjęto jako SSOT wejściowy do przyszłego planu wykonawczego:

- `PLANS/AUDYT/RAPORT_AUDYT_KOREKTY_INTERPRETACJI_METRYK_20260710.md`

Raport ma obejmować dokładnie dziesięć wskazanych pozycji oraz rozróżniać dla
każdej z nich:

- producenta i populację wejściową,
- formułę, denominator, skalę i status braku danych,
- canonical MFS od compat, shadow, logging-only i export-only,
- potwierdzony stan bieżący od kierunku przyszłej naprawy,
- kompatybilność legacy od semantyki nowego, addytywnego pola.

Ta decyzja nie zmienia kodu ani zachowania runtime. Nie zmieniono BUY/REJECT,
progów, konfiguracji, selector score, live/shadow boundary ani formatu istniejących
rekordów JSONL.

## 3. Potwierdzony kontrakt dziesięciu pozycji

1. `fee_topology_diversity_index` w canonical Sybil/MFS to liczba unikalnych
   topologii podzielona przez liczbę unikalnych próbek kupujących. Dwie próbki
   mogą dać wartość oraz degraded evidence. Coordination-risk ma osobną,
   znormalizowaną HHI-diversity i pozostaje `ExportOnly`.
2. `dev_buy_total_sol` na canonical TxIntel/MFS path jest pierwszym zaakceptowanym,
   nie-dust, zidentyfikowanym BUY deva według kolejności obserwacji TxIntel; nie
   dowodzi totalu ani sukcesu on-chain. GatekeeperBuffer compat ma odmienną,
   mocniejszą selekcję opartą m.in. o create signature. `dev_volume_ratio` to
   gross dev buy-plus-sell turnover share, nie holdings, net exposure ani total
   dev buys.
3. `same_ms_tx_ratio` w TxIntel liczy dokładnie zerowy odstęp między sąsiednimi
   timestampami, natomiast helper phase diversity używa `<50 ms`. RCE ma jeszcze
   osobną, success-only, recent-window semantykę. Nazwa bez source/population/
   window/denominator nie jest wystarczającym kontraktem.
4. `top3_volume_pct` jest legacy aliasem ratio 0..1. Preferowane pole
   `top3_signer_volume_ratio: Option<f64>` i fallback kompatybilności już istnieją;
   pozostała praca to migracja konsumentów, bez zmiany progu.
5. `flip_ratio_10s` jest hybrydą: aktywny producent używa domyślnego okna 10 s,
   progu sprzedaży 50% i maksymalnego odstępu 20 slotów. Early fingerprint jest
   zasilany przed główną deduplikacją/dust filtrem TxIntel i nie filtruje jawnie
   `success=false`, więc przyszły plan musi zdefiniować populację oraz odporność
   na ponowne dostarczenie transakcji.
6. Legacy `funding_source_concentration` to `1 - distinct_known_sources /
   known_buyers`; nie jest HHI, udziałem wolumenu ani top1 share. Jakość FSC v2
   należy czytać z `funding_source_v2.*`; aktywna V2 policy nadal konsumuje legacy,
   natomiast per-pool FSC v2 pozostaje shadow/counterfactual/export evidence.
7. `evidence_status.fsc = Clean` potwierdza wyłącznie obecność legacy scalar.
   Nie potwierdza FSC v2 readiness, coverage ani `Clean`. W bazowym
   `ghost_brain_config.toml` wymóg FSC dla V3 evidence jest jawnie wyłączony,
   ale problem pozostaje w logach/replay i w każdym profilu, który FSC wymaga.
8. `ManipulationContradictionFeatures.high_*` pozostają `false`, ponieważ
   materializer wypełnia wartości numeryczne i używa `Default` dla flag. V3
   obecnie sprawdza również odpowiadające progi numeryczne, więc nie potwierdzono
   pełnego bypassu hard-risk; fałszywe flagi nadal psują evidence i replay.
9. `reserve_velocity_sol_per_sec` jest średnią między kolejnymi account updates
   według czasu odbioru. `0.0` może znaczyć realne zero, pierwszą aktualizację,
   zerowy odstęp czasu albo bootstrap/fallback; nie jest ciągłym samplerem.
10. `buy_sell_ratio_recent` jest success-only cechą RCE z okna recent. Gdy nie ma
    selli, pole zwraca `buy_count`, więc jest nieograniczone i ma zmienny
    denominator. Powierzchnia jest obecnie logging-only.

## 4. Granice architektoniczne

Analiza objęła:

- `MaterializedFeatureSet` jako canonical decision snapshot,
- TxIntelligence i Sybil metric contracts,
- funding-source legacy/FSC v2 ownership,
- Gatekeeper V2/V3 consumer boundaries,
- DecisionLogger/replay evidence,
- Seer early fingerprint producer,
- AccountStateCore update/fallback semantics,
- aktywny profil V3 w `ghost_brain_config.toml`.

Nie zmieniono ani nie promowano:

- Solana execution, sendera, builderów i potwierdzania transakcji,
- event routingu lub session lifecycle,
- policy, thresholdów lub config defaults,
- FSC v2 do aktywnej polityki,
- RCE do policy,
- legacy lub test-only decision path,
- shadow simulation do live inclusion.

## 5. Konsekwencje dla następnego planu

Plan wykonawczy powinien być addytywny i etapowy. Minimalny kierunek obejmuje:

- P0: rozdzielić statusy `fsc_legacy` i `fsc_v2`; nie redefiniować po cichu
  istniejącego `evidence_status.fsc`,
- P0: zachować raw numeric manipulation evidence jako SSOT, a flagi stage/profile
  wyprowadzać jawnie w V3 lub zdeprecjonować nieprawdziwe `high_*`,
- P0: zdefiniować source/order/success/dedupe contract pierwszego dev buy i early
  fingerprint flip population,
- P1: dodać source/population/window/denominator metadata lub jednoznaczne pola
  dla same-ms, reserve velocity i recent buy/sell,
- P1: dodać status/interval/source-clock dla reserve velocity oraz raw buy/sell
  counts i jawny zero-sell denominator status,
- P2: zakończyć migrację `top3_volume_pct` do już istniejącego pola ratio oraz
  zachować serde/replay compatibility,
- dla każdego kroku: testy starych rekordów, parity legacy oraz dowód braku policy
  drift i braku zmiany shadow/live behavior.

Plan nie może używać `dev_volume_ratio` jako substytutu ekspozycji. Jeśli
potrzebna jest ekspozycja lub total dev buy volume, musi powstać osobny kontrakt
z własnym producerem, population i evidence status.

## 6. Pliki

Pliki utworzone lub poprawione w tym zadaniu:

- `PLANS/AUDYT/RAPORT_AUDYT_KOREKTY_INTERPRETACJI_METRYK_20260710.md`
- `docs/ADR/ADR_8D_AUDYT_KOREKTY_INTERPRETACJI_METRYK_20260710.md`

Nie zmodyfikowano plików Rust, TOML, skryptów, testów ani artefaktów runtime.

## 7. Weryfikacja

Wszystkie uruchomione testy zakończyły się wynikiem PASS:

- FTDI two-buy degraded diagnostic: 1 passed,
- coordination FTDI HHI/export-only: 1 passed,
- TxIntelligence top3 compatibility contract: 2 passed,
- Seer flip basic: 1 pasujący test passed,
- Seer flip excessive slot gap: 1 pasujący test passed,
- session materialization temporal/RCE: 1 passed,
- session materialization FSC: 1 passed,
- neutral-only legacy-versus-v2 FSC: 1 passed,
- AccountStateCore reserve units: 1 passed.

Dokładne polecenia i interpretacja wyników znajdują się w sekcji 9 raportu.
Pełny workspace test suite nie został uruchomiony. Obecny proof potwierdza lokalne
kontrakty i nie zastępuje runtime/replay evidence wymaganej po implementacji.

Po finalnym zapisie wymagane są również:

- kontrola trailing whitespace obu dokumentów,
- kontrola dokładnie dziesięciu sekcji findings,
- kontrola spójności listy metryk i zakazu jedenastego zakresu,
- kontrola `git status` i jawny proof, że nie dotknięto kodu/configu ani
  istniejących, niezwiązanych zmian użytkownika.

## 8. Scope control

Raport ani ADR nie są zgodą na:

- runtime promotion FSC v2,
- zmianę Gatekeeper policy, BUY/REJECT, score albo progów,
- usunięcie lub reinterpretację legacy JSON fields,
- aktywację RCE jako policy input,
- włączenie live behavior,
- cross-cutting refactor,
- potraktowanie testów jednostkowych jako produkcyjnego runtime proof.

Każda taka zmiana wymaga osobnego planu, kompatybilnej migracji, ADR oraz
targeted replay/shadow validation.
