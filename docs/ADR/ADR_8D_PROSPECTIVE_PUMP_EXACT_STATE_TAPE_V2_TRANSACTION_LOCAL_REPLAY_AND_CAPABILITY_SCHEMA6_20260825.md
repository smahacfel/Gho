# ADR-8D: Prospective Pump Exact-State Tape V2 — transaction-local replay i capability schema 6

**Data:** 2026-08-25

**Status:** IMPLEMENTED / FINAL CENSUS PASS / SCHEMA-6 QUALIFIED / OUTCOME-BLIND EXPORT PASS

**Typ:** ADR-8D / prospective PRXTAPE3 / offline exact-state qualification

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie jest dostępny w tym
> środowisku. Dokument zachowuje lokalny układ ADR-8D używany przez istniejące
> ADR-y V2.

## D0. Potwierdzony problem

Zachowany dziesięciominutowy PRXTAPE3 jest kompletnym, immutable raw source,
ale ostatnia kwalifikacja schema 5 pozostała `Blocked`:

```text
global successful rooted candidates = 23 908
prospective scoped denominator       = 8 408
exact                                = 7 843
explicit non-exact                   =   565
coverage                             = 932 802 ppm
```

Dwie reguły kwalifikatora były zbyt szerokimi ograniczeniami implementacji,
a nie dowodem braku primary evidence:

1. minimum V1.1 `30 min OR 10 000 mutations` było obliczane z denominatora
   prospective-birth scope, mimo że plan definiuje gałąź mutation-count jako
   globalne, zreconciliowane universe successful rooted candidates;
2. każda transakcja z więcej niż jedną reserve/dependency mutation była
   odrzucana bez próby wykorzystania zachowanych strict Event-CPI i finalnego
   same-signature account anchoru.

Ponadto current ProgramData wykonał kilka zamkniętych ABI spellings, których
publiczny vendored IDL nie wyraża literalnie: dodatkowe końcowe booleany dla
wybranych trade variants oraz brak terminalnego `OptionBool` w `create_v2`.
Raw zachowuje pełne payloady i strict event evidence; nie wolno jednak
przypisywać niewymienionym bajtom ani nieobecnemu argumentowi wymyślonej
semantyki.

## D1. Decyzja — jedna production authority dla census i qualifiera

Feasibility census klasyfikuje dokładnie 565 residual candidates wskazanych
przez SHA-pinned baseline coverage. Populacja jest wybierana z baseline, a nie
z wyniku nowego parsera, aby poprawiony decoder nie mógł usunąć własnych
historycznych residuali z dowodu.

Ostateczna klasyfikacja census pochodzi z tej samej funkcji produkcyjnego
transaction-local replay, której używa `qualify`. Census-only modele przejścia
mogą pozostać diagnostyką, ale nie są authority dla `recoverable` ani dla
projected capability. Raport wymaga jednocześnie:

```text
baseline residual conservation      = 402 + 163 = 565, exactly once
recoverable residuals               >= 557
projected denominator conservation  = exact + explicit non-exact
projected coverage                  >= 999000 ppm
unknown/malformed/global/unscoped   = 0
```

## D2. Zamknięte compatibility grammars

Standardowy strict Borsh decoder pozostaje pierwszą i domyślną ścieżką.
Po jego niepowodzeniu dozwolone są wyłącznie literalne, zamknięte formy
potwierdzone przez immutable census:

```text
buy_exact_quote_in_v2 = dwa u64 + dokładnie jeden bool
buy_v2                 = dwa u64 + dokładnie jeden bool
sell                   = dwa u64 + jeden albo dwa bool
create_v2              = pełny prefix do is_mayhem_mode,
                         bez terminalnego OptionBool
```

Każdy dodatkowy suffix byte musi być `0|1`, jest zużywany przez grammar, ale
nie dostaje nazwy argumentu ani state authority. Brakujące
`is_cashback_enabled` nie jest wstawiane do argument mapy. Inna długość,
invalid bool, inna instrukcja albo drift kształtu IDL pozostają fail-closed.

## D3. Produkcyjny transaction-local exact replay

Kandydaci są grupowani po literalnej identity BondingCurve i sortowani po
pełnym instruction locatorze. Grupa jednoelementowa zachowuje dotychczasowy
exact-anchor kontrakt. Grupa wieloelementowa może zostać exact wyłącznie, gdy:

1. każda jej mutacja to manifest-pinned `SupportedExactCreate` albo
   `SupportedExactTrade`;
2. każdy parent ma exact payload i account vector;
3. każdy parent ma dokładnie jeden strict, immediate-parent-bound CreateEvent
   albo TradeEvent; Event-CPI nadal wymaga Pump self-CPI, canonical PDA,
   właściwego path/stack i pełnego Borsh consumption;
4. trade side, token amount, quote amount oraz parent limit/budget są zgodne;
5. checked reserve deltas każdego TradeEvent bitowo odtwarzają poprzedni stan;
6. identity/carry fields nie zmieniają się bez transition evidence;
7. ostatni stan bitowo zgadza się z unique same-signature final account anchor.

Dla `Create -> Trade` CreateEvent dostarcza trzy jawne rezerwy, supply,
creator, quote mint oraz flags. Brakujące w CreateEvent `real_quote_reserves`
jest wyprowadzane wyłącznie jako jednoznaczny preimage bezpośrednio następnego
strict TradeEvent. Następnie cała sekwencja musi dojść do finalnego account
anchora. Nie ma protocol defaultu, carry-forward, RPC ani imputacji.

Domyślnie znana niewspierana mutacja tej samej curve nadal blokuje grupę.
Wyjątek jest zamknięty do trzech census-confirmed, anchor-observed migracji:

```text
migrate
migrate_v2
migrate_bonding_curve_creator
```

Każda z nich wymaga exact parent payload/account vector, dokładnie jednego
strict immediate-parent Event-CPI właściwego wariantu, literalnego związania
identity eventu z rolami parenta oraz dwóch primary account states: exact
pre-state i unique same-signature final anchor. `migrate` i `migrate_v2` nie
dostają wymyślonego równania rezerw — exactness oznacza tutaj wyłącznie dwa
bezpośrednio zaobserwowane, kompletne stany powiązane z jedną, jednoznaczną
mutacją. Legacy `migrate` wymaga dodatkowo literalnego zero-pubkey
`quote_mint`, które current ProgramData emituje dla native-SOL migration;
`migrate_v2` wymaga zgodności eventowego `quote_mint` z rolą parenta.
`migrate_bonding_curve_creator` dopuszcza wyłącznie zmianę `creator` zgodną z
`old_creator/new_creator`; wszystkie pozostałe pola BondingCurve muszą być
bitowo zachowane. Migracja może być tylko ostatnią mutacją danej curve w
transaction-local chain.

Inna znana niewspierana mutacja nie dostaje transition semantics ani
exactness. Jej identity curve/mint może pochodzić wyłącznie z literalnych ról
`bonding_curve` i `mint|base_mint` przypiętego IDL, tylko dla
scope/denominator conservation.

## D4. Capability schema 6

Schema 6 oblicza `QualificationRunBelowMinimum` z:

```text
cohort_elapsed >= 1 800 000 ms
OR
global successful rooted candidate count >= 10 000
```

Scoped prospective denominator pozostaje authority coverage, ale nie zastępuje
globalnego mutation-count universe. Exporter i artifact revalidation ponownie
obliczają flagę z receipt-bound global counter. Schema 5 pozostaje
niekompatybilna i jest odrzucana; nie jest retroaktywnie reinterpretowana.

## D5. Zakres wyłączony

Zmiana nie uruchamia i nie dodaje:

- nowego capture'u, Yellowstone ani provider I/O;
- GPA, snapshotu, RPC backfillu, imputacji lub carry-forward;
- obniżenia `999000 ppm`, denominator shrink ani manual override;
- zmian V1/GO-D, aktywnego Seer, OracleRuntime, Gatekeepera, execution lub
  strategii;
- outcome'ów, PnL, SELECTED/REST ani decyzji tradingowej.

Po lokalnych testach i self-review dozwolona jest jedna create-new offline
qualification zachowanego raw. `export-window` może zostać uruchomiony tylko
dla schema-6 `Qualified` i nadal publikuje wyłącznie outcome-blind
150000/90000 ms windows.

## D6. Finalny feasibility census na preserved raw

Świeży, create-new census schema 4 został wykonany na całym immutable
PRXTAPE3 i zakończył się kodem `0` po `1435.39 s`. Raport:

```text
/tmp/pump-v2-multi-mutation-feasibility-census-
  pump-exact-state-v2-1787539185686-2720125.json

SHA-256 = 4340e4a5e1f115928570b2c403dd57054977922be7e267c4926a5b4eee0114a0
bytes   = 3 054 298
lines   = 71 525
mode    = 0600
```

Receipt-bound wynik:

```text
baseline residual population                565
classified exactly once                     565
recoverable by production replay             565
required minimum recovery                     557
unclassified / duplicate                       0 / 0

multi-mutation recovered                   402 / 402
inventory/layout recovered                 163 / 163
production replay parity                        true

projected scoped denominator                    8 458
projected exact mutations                        8 453
projected explicit non-exact                         5
projected exact coverage                      999 408 ppm
required exact coverage                       999 000 ppm
required exact mutation count                     8 450

global unknown / malformed / dependency           0 / 0 / 0
unscoped / scope-incomplete                        0 / 0
feasibility gate                                  PASS
```

Populacja residuali pozostaje przypięta do baseline receipt SHA-256
`43737a29d3a194571e65b6a3b5d6a41767079755aac6c21a7b9c71a7ff092d74`.
Census nie zmienił raw, nie pobrał danych zewnętrznych i użył tej samej
produkcyjnej funkcji replayu, którą wywoła kwalifikator schema 6.

Raport został wykonany przed D9, która zmienia wyłącznie tryb pliku przyszłych
raportów. D9 nie zmienia wejść klasyfikatora, produkcyjnej funkcji replayu ani
któregokolwiek z powyższych liczników. Zachowany raport został następnie
ustawiony owner-private (`0600`) bez zmiany jego bajtów ani SHA-256.

## D7. Schema-6 qualification i outcome-blind export

Po pełnej locked/offline macierzy, neutralnym self-review i świeżym release
buildzie wykonano jedną skuteczną create-new qualification zachowanego raw.
Pierwsze wywołanie zatrzymało się przed materializacją na literalnej bramce
storage; nie utworzyło outputu ani `.partial`. Po usunięciu wyłącznie
odtwarzalnego `target/debug` ponowiono identyczną operację tą samą release
binarką:

```text
materializer executable SHA-256  = 6ac316a9a6d6855805d722a13baaf09d287ac1902f892a4384c1da5e0b2d3b1c
materializer executable bytes    = 11 861 680
materializer executable mode     = 0700

capability schema                = 6
status                           = Qualified
blockers                         = []
global rooted candidates         = 23 908
scoped denominator               = 8 458
exact mutations                  = 8 453
explicit non-exact               = 5
exact coverage                   = 999 408 ppm
required coverage                = 999 000 ppm
exact trajectories               = 8 453
trades with both states          = 8 170
exact births                     = 238
unknown/malformed/global blockers= 0
scope/denominator/occurrence     = reconciled
```

Exact artifact:

```text
/protected/research/exact-v2/
  pump-exact-state-v2-1787539185686-2720125-schema6-transaction-local-replay

exact_state_capability_v2.json SHA-256 = 98832e4039e3f599ce20fd3c7f977bfdc5006b839c686aec4714e8fae7fdb524
manifest_v2.json SHA-256               = d6f310c233015477d36f910417854b0150c4c2e385b082d233fe9a59359609d8
partial files                            = 0
directory/files mode                     = 0700 / 0600
```

Następnie ta sama binarka ponownie zweryfikowała raw, semantics, receipt,
manifest i exact JSONL, po czym opublikowała wyłącznie outcome-blind windows:

```text
/protected/research/outcome-blind-v2/
  pump-exact-state-v2-1787539185686-2720125-schema6-transaction-local-replay

exported births                  = 238
complete windows                 = 147
observation                      = 150 000 ms
forward availability             = 90 000 ms
time authority                   = observed_ingress_monotonic_ms
window JSONL SHA-256             = 7e6bf14f6949b3e65ee548b20489a3aa4112fd849b6a1841800fd531ff4c1983
window manifest SHA-256          = 1bbc44fec68685bcb28ca8b73d9e391e86b2e28e312b146a48e2e5396320638d
partial files                    = 0
```

Żaden exported row nie zawiera PnL, outcome, entry/exit, score,
SELECTED/REST, Gatekeeper ani execution. Pipeline pozostaje zatrzymany przed
outcome'ami strategii.

## D8. Deterministyczne uprawnienia fixture'ów kwalifikatora

Productionowy writer exact output słusznie wymaga, aby jego istniejący katalog
nadrzędny był owner-private (`0700`). Publiczne fixture'y V2 nie mogą jednak
uzależniać tego kontraktu od `umask` procesu uruchamiającego testy. Fixture
raw-to-qualify oraz dwa bezpośrednie testy writera ustawiają więc jawnie
`0700` na swoim tymczasowym katalogu authority przed pierwszą próbą
publikacji.

Nie osłabia to walidacji produkcyjnej, nie zmienia trybu outputu i nie
akceptuje katalogu `0755`; usuwa wyłącznie niedeterministyczność harnessu,
która maskowała właściwe asercje przy standardowym `umask 022`.

## D9. Prywatna publikacja raportu feasibility census

Finalny read-only census ujawnił, że `OpenOptions::create_new(true)` chronił
przed nadpisaniem, lecz domyślnie dziedziczył umask procesu. Raport zawiera
raw-derived locators i anchor evidence, dlatego od teraz jego publikacja
wymusza na Unix `0600` niezależnie od umask. Regresja sprawdza oba warunki:
owner-private mode oraz atomową odmowę nadpisania istniejącego raportu.

Nie zmienia to logiki census, kodu replayu ani raw tape. Dotyczy wyłącznie
właściwości przyszłej publikacji diagnostycznej; istniejący raport z D6 ma
jednorazowo ustawiony tryb owner-private bez zmiany treści lub hashy.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```
