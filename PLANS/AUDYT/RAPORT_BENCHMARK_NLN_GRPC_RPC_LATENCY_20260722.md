# Raport: krótki benchmark latency NLN gRPC/RPC i kontrolowany submit Jito — 2026-07-22

## Werdykt

### Aktualizacja: rzeczywista ścieżka submitu, 2026-07-22T15:33:40.775Z

Po osobnej zgodzie wykonano po jednej kontrolowanej transakcji na dwóch niezależnych trasach: bezpośredni sendBundle do Jito Frankfurt i standardowy sendTransaction do NLN RPC. Każda miała inny podpis i Memo z nazwą lane, ten sam inline Jito tip 0.002 SOL oraz ten sam priorytet 25,000 µlamports/CU przy limicie 50,000 CU. Nie kupowała tokena, nie uruchamiała Ghosta i nie miała retry.

Obie transakcje landed i są finalized bez błędu. Direct Jito zwrócił ACK 5.54 ms i landed w slocie 434535725; NLN zwrócił ACK 11.42 ms i landed/finalized w slocie 434535734. To potwierdza oba konkretne wywołania z tej maszyny i tego klucza, lecz nie ustanawia rankingu end-to-end latency: są to pojedyncze, sekwencyjne próby w innych slotach. Standardowy reply NLN nie zwraca bundle_id, więc test nie dowodzi, że NLN zmirrorował ją do Jito.

Z perspektywy tej maszyny testowej NLN ma **bardzo niskie opóźnienia na rozgrzanej ścieżce transportowej**:

- Yellowstone gRPC `Ping`: **p50 7.46 ms, p95 10.25 ms** (`n=7`);
- HTTP RPC `getSlot(processed)`: **p50 10.13 ms, p95 25.05 ms** (`n=6`);
- HTTP RPC `getLatestBlockhash(processed)`: **p50 10.08 ms, p95 13.92 ms** (`n=6`).

W krótkim strumieniu transakcji Pump.fun subskrypcja `processed` była gotowa po **8.46 ms**, a pierwsza pasująca wiadomość przyszła po **17.25 ms** od wysłania żądania subskrypcji. Pobrano 8/8 wymaganych aktualizacji w 287.83 ms, bez uruchamiania bota i bez wysyłania jakiejkolwiek transakcji.

To jest pozytywny wynik dla transportu i odbioru danych. Nie jest to jednak dowód „X ms od łańcucha”, ani dowód latencji do Jito. Aktualna wersja proto Yellowstone w tym checkoutie nie dostarcza timestampu wysłania aktualizacji przez provider, a żadna transakcja ani bundle nie została celowo wysłana.

Najważniejsza obserwacja operacyjna: początkowa gęsta seria zapytań RPC została ograniczona przez NLN odpowiedzią `-32005 Rate limited`. Wynik końcowy pochodzi wyłącznie z powtórzonej, spokojnej próby z jednym procesem i tempem około 1 żądania HTTP/s. Nie wolno interpretować tego jako potwierdzenia przepustowości konta ani jako latency pod wysokim obciążeniem.

## Zakres, bezpieczeństwo i granice testu

| Właściwość | Wartość |
| --- | --- |
| Czas rozpoczęcia zaakceptowanego przebiegu | `2026-07-22T13:17:14.602Z` |
| gRPC | `https://grpc.nln.clr3.org:443` |
| RPC | `https://rpc.nln.clr3.org` |
| Autoryzacja obu endpointów | `x-api-key`; klucz był tylko zmienną procesu i nie został zapisany do repo, raportu ani logów |
| Geyser commitment | `processed` |
| Filtr Pump.fun | transakcje zawierające `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P` |
| Uruchamiany komponent Ghost | żaden — użyty był niezależny przykład `seer` |
| Efekt on-chain / Jito | faza odczytowa: żaden; późniejszy, osobno zatwierdzony addendum: dokładnie dwa podpisane tip-only probes, opisane w sekcji „Kontrolowany submit” |

Test stosował `Instant` (zegar monotoniczny) dla każdego mierzonego odcinka. Tym samym liczby nie zależą od synchronizacji zegara hosta z serwerem NLN.

NLN dokumentuje dla tego gRPC endpoint `grpc.nln.clr3.org:443` oraz autoryzację nagłówkiem `x-api-key`; dla RPC dokumentuje ten sam nagłówek i endpoint `https://rpc.nln.clr3.org`. [Yellowstone gRPC Overview — NLN](https://nolimitnodes.com/docs/api-reference/grpc/overview), [NLN RPC Nodes](https://nolimitnodes.com/products/rpc-nodes)

## Metodologia

### gRPC — transport i odbiór Pump.fun

1. **Cold connect + authenticated Ping**: świeży klient TLS/HTTP/2, połączenie i poprawny `Ping` w jednym pomiarze.
2. **Warm Ping**: seryjne `Ping` na jednym, rozgrzanym kanale.
3. **Pump.fun stream**: świeże `Subscribe` Yellowstone z jednym filtrem `transactions.account_include=[Pump.fun]` i `commitment=processed`; pomiar od wywołania `Subscribe` do otwarcia streamu i do pierwszego komunikatu, następnie odbiór ośmiu komunikatów.

Celowo nie subskrybowano account updates, block-meta ani Entry: benchmark ma mierzyć transportowy odbiór transakcji Pump.fun, a nie obciążenie pełnej topologii Seera. Aktywny klient nadal może mieć dodatkowe koszty parsera, kanałów bounded, Event Busa oraz konsumentów; nie są one częścią tego wyniku.

### RPC — normalna ścieżka odczytu przed transakcją

1. **Cold connect + `getSlot(processed)`**: nowy klient HTTP, połączenie TLS i pełne żądanie JSON-RPC.
2. **Warm `getSlot(processed)`**: odczyt bieżącej świeżości.
3. **Warm `getLatestBlockhash(processed)`**: odczyt potrzebny w typowej ścieżce budowania transakcji.
4. **Slot parity**: równolegle `GetSlot(processed)` po gRPC i `getSlot(processed)` po HTTP RPC. Mierzony jest osobno RTT obu wywołań oraz różnica zwróconych slotów `gRPC - RPC`.

Próba była celowo mała: `n=2` dla cold, `n=6–7` dla warm i `n=5` dla slot parity. Jest wystarczająca do szybkiej oceny operacyjnej aktualnej trasy z tej maszyny, ale nie do SLO, porównania regionów ani analizy ogonów pod obciążeniem.

## Wyniki zaakceptowanego przebiegu

### gRPC

| Pomiar | n | min | p50 | p95 | p99 | max | średnia |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Cold TLS/HTTP2 connect + `Ping` | 2 | 34.98 ms | 34.98 ms | 69.64 ms | 69.64 ms | 69.64 ms | 52.31 ms |
| Warm authenticated `Ping` | 7 | 6.01 ms | **7.46 ms** | **10.25 ms** | 10.25 ms | 10.25 ms | 7.80 ms |

`GetVersion` odpowiedział poprawnie; endpoint ujawnił usługę `richat 11.0.0` z proto `12.4.0`. Jest to dowód, że benchmark połączył się z usługą Geyser/Yellowstone, a nie jedynie z otwartym portem TCP.

### gRPC Pump.fun stream

| Pomiar | Wynik |
| --- | ---: |
| Czas `Subscribe` → stream ready | **8.46 ms** |
| Czas wysłania `Subscribe` → pierwsza transakcja Pump.fun | **17.25 ms** |
| Odebrane transakcje / cel | **8 / 8** |
| Łączny czas pobrania | 287.83 ms |
| Unikalne sloty w tej małej próbce | 1 |
| Inter-arrival p50 / p95 | 41.62 ms / 103.78 ms |
| Zakończenie | osiągnięto cel, nie timeout |

**Prawidłowa interpretacja:** 17.25 ms jest czasem dostarczenia pierwszego już dostępnego zdarzenia po założeniu subskrypcji. Nie jest czasem powstania transakcji ani czasem od wykonania jej przez walidator. Yellowstone proto 1.14 używane w checkoutie zawiera dla `SubscribeUpdateTransaction` slot i payload transakcji, ale nie zawiera czasu emitowania po stronie providera; dlatego nie istnieje niezależny znacznik początkowy do uczciwego wyliczenia one-way latency.

### HTTP RPC

| Pomiar | n | min | p50 | p95 | p99 | max | średnia |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Cold TLS/HTTP connect + `getSlot` | 2 | 83.85 ms | 83.85 ms | 93.50 ms | 93.50 ms | 93.50 ms | 88.67 ms |
| Warm `getSlot(processed)` | 6 | 9.56 ms | **10.13 ms** | **25.05 ms** | 25.05 ms | 25.05 ms | 12.83 ms |
| Warm `getLatestBlockhash(processed)` | 6 | 9.90 ms | **10.08 ms** | **13.92 ms** | 13.92 ms | 13.92 ms | 11.31 ms |

### Freshness gRPC vs RPC

| Pomiar równoległy, `processed` | Wynik |
| --- | ---: |
| gRPC `GetSlot` p50 / p95 (`n=5`) | 8.77 ms / 17.30 ms |
| RPC `getSlot` p50 / p95 (`n=5`) | 10.03 ms / 17.36 ms |
| `gRPC slot - RPC slot`: min / p50 / max | **0 / 0 / 0 slotów** |

W pięciu równoległych próbkach oba endpointy widziały identyczny `processed` slot. Jest to pozytywna obserwacja świeżości ich kontrolnych odczytów; nie dowodzi identycznego czasu dostarczenia pojedynczej transakcji, gdyż oba wywołania dotyczą stanu sieci, a nie tego samego eventu z niezależnym timestampem źródłowym.

## Co można realnie założyć w runtime

1. **Gdy kanał już jest otwarty**, dla małych kontrolnych RPCs z tej lokalizacji realistyczny budżet transportu do NLN to obecnie około **7–14 ms w typowym przypadku**, z obserwowanym ogonem około **10–25 ms** w tej małej próbie.
2. **Przy nowym połączeniu** trzeba doliczyć TLS/HTTP2: obserwowane około **35–70 ms** dla gRPC i **84–94 ms** dla HTTP RPC. Runtime powinien utrzymywać połączenia, a nie tworzyć je na gorącej ścieżce.
3. **Dla danych Pump.fun** stream był operacyjny i dostarczył osiem aktualizacji `processed` od razu po subskrypcji. Nie ma dowodu na absolutne opóźnienie łańcuch → Ghost; aby taki benchmark był możliwy, provider musiałby udostępnić timestamp ingest/emit w payloadzie albo potrzebny byłby zewnętrzny, zsynchronizowany marker tej samej transakcji.
4. **Dla RPC** wyniki dotyczą odczytów. Nie należy z nich wyprowadzać opóźnienia wysłania transakcji, potwierdzenia, ani inclusion.

## Jito: odpowiedź precyzyjna dla pierwszej, odczytowej fazy

**Pierwsza, odczytowa faza sama w sobie nie pozwalała uczciwie powiedzieć, jakie jest opóźnienie „do Jito”.** Została później uzupełniona kontrolowanym testem submission/landing opisanym bezpośrednio przed sekcją rate-limit. Nie zmienia to granicy metrologicznej: nawet po teście nie mamy dokładnego wall-clock ACK do execution, tylko local ACK, chain slot i chwilę wykrycia statusu.

NLN deklaruje, że jego ścieżka `sendTransaction` ma leader-aware forwarding i jest mirroryzowana do Jito Block Engine. [Opis routingu RPC NLN](https://nolimitnodes.com/products/rpc-nodes) To jest jednak opis providera, a nie lokalny pomiar dla tego klucza i tej maszyny.

Zmierzony `getLatestBlockhash` p50 ~10 ms oznacza jedynie szybki odczyt do RPC. Pełna ścieżka Jito ma dodatkowe, niezależne od tego testu odcinki:

```text
podpisanie i serializacja
→ HTTP RPC / forwarder NLN
→ Block Engine / TPU
→ leader slot
→ inclusion
→ status / confirmation
```

Do wiarygodnego benchmarku Jito potrzebne są osobno: zgoda na kontrolowany, podpisany no-op lub bundle, wyraźnie ustalony endpoint/protokół Jito, czas lokalnego `send`/ack oraz niezależne potwierdzenie landed slotu. Bez tej zgody test nie wysyła transakcji — i słusznie nie udaje wyniku inclusion latency.

## Kontrolowany submit Jito Frankfurt i NLN RPC

### Kontrakt testu

| Właściwość | Wartość |
| --- | --- |
| Czas uruchomienia | 2026-07-22T15:33:40.775Z |
| Payer | testowy keypair jawnie wskazany przez operatora; fingerprint 9MCk…vbaw; klucz prywatny ani ścieżka nie trafiły do repo |
| Saldo przed / po | 0.047172000 SOL / 0.043159500 SOL |
| Lane A | Jito Frankfurt, endpoint /api/v1/bundles, sendBundle z jednym podpisanym TX |
| Lane B | NLN RPC, standardowy sendTransaction z skipPreflight=true, maxRetries=0, minContextSlot |
| Transakcja | ComputeBudget 50k CU i 25,000 µlamports/CU, signed Memo lane, inline transfer do konta tipowego Jito |
| Tip / max priority fee | 2,000,000 lamportów / 1,250 lamportów na lane |
| Liczba submitów | dokładnie 1 na lane; zero retry, resubmission i automatycznej eskalacji tipu |
| Dotknięta funkcjonalność Ghosta | żadna: osobny przykład seer, bez importu ani uruchomienia runtime |

Jito getTipAccounts zwrócił bieżące konto tipowe; jedno konto wybrano losowo i wykorzystano to samo konto dla obu lane. Tip był instrukcją w tej samej transakcji co Memo, nie osobną transakcją bundle. W lane NLN taki tip może być kosztem bez korzyści, jeśli TX wykonuje leader nie-Jito. To świadomy koszt równości parametrów testu, nie zalecenie produkcyjne. [Jito Low Latency Transaction Send — Tips](https://docs.jito.wtf/lowlatencytxnsend/)

Nie użyto jednej, identycznej sygnatury na obu trasach. Pierwszy landing unieważniłby drugi wynik jako AlreadyProcessed i nie pozwolił atrybuować wyniku do lane. Transakcje są równoważne ekonomicznie, ale różnią się Memo/signature.

### Wynik submission i landing

| Metryka | Direct Jito Frankfurt sendBundle | NLN sendTransaction |
| --- | ---: | ---: |
| ACK HTTP JSON-RPC | **5.54 ms** | **11.42 ms** |
| Payload ACK | bundle_id | ta sama sygnatura co podpis lokalny |
| Slot referencyjny przed submit, getSlot | 434535719 | 434535730 |
| Landed slot | **434535725** | **434535734** |
| Landing slot minus pre-submit slot | +6 slotów | +4 sloty |
| Pierwszy automatyczny chain check | 1,154.21 ms: inflight Invalid, później rozstrzygnięty | 1,104.91 ms: confirmed |
| Wynik końcowy chain | finalized, err=null | finalized, err=null |
| Fee meta.fee | 6,250 lamportów | 6,250 lamportów |
| Inline tip | 2,000,000 lamportów | 2,000,000 lamportów |
| Całkowity potwierdzony koszt lane | 2,006,250 lamportów | 2,006,250 lamportów |

Dowody identyfikujące:

| Lane | Sygnatura | Identyfikator bundle |
| --- | --- | --- |
| Jito Frankfurt | 29wHzWgwSZ82AZ8o5K4c6tEVrqHd7zdgHAtN2cAgUunx8yoY27ZRUZJ7YPd4yxoNnsQybVfkvt26HyUZehpyXAb3 | 10ec84e0ffedfce7af0ffec170f0647a3f227677f03d275ef443dc875849b5ad |
| NLN RPC | wG2FzWyitJuXxyj1CK5AsFPd5vf9SxfZZaYFT4cXb85boqptVmauRkpPyRtokoQro43nyMfnLfjQcrjf3r7dA89 | brak: zwykły sendTransaction nie zwraca bundle_id |

Saldo spadło dokładnie o 4,012,500 lamportów, czyli 2 razy (2,000,000 tip plus 6,250 fee). Jest to niezależna kontrola kosztu, zgodna z obu odpowiedziami API.

### Krótkie rozszerzenie: finality i koszt do n=5

Po pierwszej parze wykonano jeszcze cztery identyczne pary, nadal z tipem 0.002 SOL, bez retry i bez eskalacji. Niezależna reconciliation historyczna portfela wykazała dokładnie dziesięć lane-specific Memo probes:

| Lane | Finalized signatures | Landed slots |
| --- | ---: | --- |
| Direct Jito code path | 5 / 5, err=null | 434535725, 434537400, 434537427, 434537454, 434537481 |
| NLN standard sendTransaction | 5 / 5, err=null | 434535734, 434537412, 434537440, 434537467, 434537493 |

Łączny potwierdzony koszt pięciu par wyniósł 20,062,500 lamportów (0.020062500 SOL), a końcowe saldo testowego keypaira to 27,109,500 lamportów (0.027109500 SOL). To jest **5/5 finalized dla obu lane**, lecz nie jest pięciopunktowym rozkładem ACK: trwały raport ACK zachował się tylko dla pierwszej pary, dlatego tabeli 5.54 ms i 11.42 ms nie wolno przedstawiać jako p50/p95.

Dla czterech rozszerzających Jito bundle, późne getBundleStatuses zostało wykonane już po oknie recent-status i zwróciło null. Nie jest to failure ani dowód braku landingu: on-chain signatures są finalized, ale provider-level bundle attribution z bundle_id pozostaje w tym raporcie twardym dowodem tylko dla pierwszej pary. To ograniczenie jest celowo jawne; do przyszłej serii harness musi zapisywać od razu wyłącznie zredukowany rekord ACK i bundle_id do kontrolowanego raportu, bez raw transaction.

### Istotna rozbieżność Jito: inflight Invalid nie oznacza braku landingu

Pierwsze getInflightBundleStatuses dla bundle Jito zwróciło status Invalid oraz landed_slot=null. Nie było to jednak dowodem failure:

1. niezależne getSignatureStatuses z NLN, z searchTransactionHistory=true, zwróciło dla tej sygnatury finalized, err=null i slot 434535725;
2. bezpośrednie getBundleStatuses z Jito zwróciło ten sam bundle_id, identyczną sygnaturę, slot 434535725, confirmation_status=finalized oraz err={Ok:null};
3. saldo i meta.fee potwierdzają wykonanie tip transferu.

Status inflight jest lifecycle cache, nie samotnym werdyktem transakcyjnym. Harness został po tej obserwacji poprawiony: przy Inflight Invalid albo Failed wykonuje teraz getBundleStatuses, zanim zakończy lane jako failure. [Jito Bundle Status APIs](https://docs.jito.wtf/lowlatencytxnsend/)

### Co wynik mówi, a czego nie mówi

**Potwierdzone:**

- Jito Frankfurt przyjął single-transaction bundle, wystawił bundle_id i bundle landed/finalized;
- NLN przyjął standardowy sendTransaction, którego podpis landed/finalized;
- lokalny ACK jednej próby był krótszy dla Jito: 5.54 ms wobec 11.42 ms;
- oba lane poniosły identyczny, jawny koszt tip plus fee.

**Niepotwierdzone:**

- dokładny czas ACK do execution: polling był celowo ograniczony do około jednego żądania na sekundę, więc observed confirmed jest górną granicą obserwacji;
- przewaga Jito nad NLN: pojedyncze, sekwencyjne próby nie dają p50/p95 ani nie kontrolują leadera/slotu;
- że NLN faktycznie użył Jito Block Engine: nie ma bundle_id, x-bundle-id, regionu ani bundleOnly;
- ochrona order-flow/revert protection dla standardowego NLN sendTransaction;
- zachowanie na prawdziwym buy pump.fun z innym CU, writable accounts, contention i slippage.

Wniosek operacyjny: **direct Jito Frankfurt jest jedyną przetestowaną trasą z jednoznacznym bundle_id i dowodem bundle statusu.** NLN sendTransaction może być dodatkowym, best-effort lane submission, lecz po tym teście nie może zastąpić direct Jito tam, gdzie wymagane są bundleOnly/revert semantics lub atrybucja Jito. Wysyłanie dwóch różnych prawdziwych BUY wymaga osobnego kontraktu idempotency i polityki partial outcome; nie wolno przenosić mechanizmu tego testu wprost do execution.

Jito dokumentuje, że sendBundle ACK oznacza odebranie bundle, nie gwarancję landingu, oraz że getBundleStatuses daje slot i confirmation. Solana dokumentuje analogicznie, że sendTransaction jest przyjęciem przez RPC, a stan trzeba rozstrzygać getSignatureStatuses. [Jito Low Latency Transaction Send](https://docs.jito.wtf/lowlatencytxnsend/), [Solana sendTransaction](https://solana.com/docs/rpc/http/sendtransaction), [Solana getSignatureStatuses](https://solana.com/docs/rpc/http/getsignaturestatuses)

### Odtwarzalny harness

Dodano odizolowany przykład off-chain/components/seer/examples/nln_jito_submission_benchmark.rs. Wymaga jawnego --execute, NLN_BENCHMARK_API_KEY oraz JITO_PROBE_KEYPAIR_PATH; sekret ani keypair nie są argumentami CLI. Domyślny tip wynosi 0.002 SOL. Tip 0.003–0.004 SOL wymaga dodatkowo --tip-lamports oraz --allow-escalated-tip, więc nie może nastąpić automatycznie. Narzędzie jest rate-paced, ma jeden submit na lane i zapisuje na stdout raport bez raw transaction bytes lub sekretów.

Walidacja po korekcie statusu inflight:

    cargo fmt --all -- --check                                  PASS
    cargo check -p seer --example nln_jito_submission_benchmark PASS
    cargo run -p seer --example nln_jito_submission_benchmark -- --help PASS
    git diff --check                                            PASS

## Rate limit: wynik operacyjny, nie latency

Wstępny, **odrzucony** przebieg wykonał serię wielu odczytów w krótkim oknie i dostał `RPC error -32005: Rate limited`. Nie jest on ujęty w tabelach latency.

Wnioski:

- aplikacja powinna ograniczać pomocnicze RPC refresh/retry i współdzielić odczyty blockhash/slot między konsumentami;
- nie wyprowadzamy z pojedynczego `-32005` dokładnego limitu planu, bo nie znamy algorytmu bucketu ani stanu wcześniejszego zużycia klucza;
- przed testem obciążeniowym trzeba uzyskać od NLN potwierdzony limit dla tego klucza/plan-u albo użyć osobnego klucza benchmarkowego.

## Implementacja i odtwarzalność

Dodano izolowany przykład:

`off-chain/components/seer/examples/nln_latency_benchmark.rs`

Nie importuje on runtime Ghosta, nie zmienia configu i nie zapisuje sekretu. Klucz jest wymagany wyłącznie z `NLN_BENCHMARK_API_KEY` i nie jest akceptowany jako argument CLI.

Przykład ponownego uruchomienia:

```bash
export NLN_BENCHMARK_API_KEY='wartosc-tylko-w-lokalnym-shellu'
cargo run -p seer --example nln_latency_benchmark -- --stream-events 12 --stream-timeout-secs 20
unset NLN_BENCHMARK_API_KEY
```

Przed finalnym przebiegiem wykonano:

```text
cargo fmt --all -- --check                         PASS
cargo check -p seer --example nln_latency_benchmark PASS
cargo build -p seer --example nln_latency_benchmark PASS
git diff --check                                   PASS
```

Build przechodzi z istniejącymi, niezwiązanymi ostrzeżeniami `ghost-core`, `trigger` i `seer`; nowe narzędzie nie wprowadziło własnego warningu ani nie zmieniło zachowania runtime.

## Niewykonane działania i następny krok

Nie zmieniono:

- Gatekeeper, `MaterializedFeatureSet`, BUY/REJECT ani selector score;
- konfiguracji, progów, routingów produkcyjnych i shadow/live boundary;
- ścieżki TX/Jito/live;
- schema/logów runtime Ghosta.

Jeżeli potrzebna będzie liczba dla Jito, to powinien powstać **osobny, wyraźnie zatwierdzony** test submission/landing. Nie powinien być doklejany do benchmarku odczytowego ani uruchamiany przeciwko prawdziwej strategii.
