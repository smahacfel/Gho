# RAPORT: Helius Sender vs Direct Jito gRPC vs NLN sendBundle — 2026-07-23

## 1. Krótki werdykt

**Końcowa decyzja operacyjna: HELIUS PRIMARY, NLN CANDIDATE REDUNDANT LANE.**

W kontrolowanym runie rzeczywista aktywna ścieżka Ghosta — LiveTxSender::send_transaction(...) do Helius Sender Frankfurt — miała najszybszą pierwszą obserwację processed oraz mniejszy dystans slotowy niż Direct Jito i NLN. NLN miał najszybszy provider ACK i pełną, lepszą od Direct Jito attribution przez bundle_id oraz getBundleStatuses, ale sam ACK nie jest powodem do zmiany primary lane. Wszystkie trzy lane osiągnęły 3/3 finalized z identycznym kosztem.

To nie jest dowód produkcyjnego SLO ani przewagi dla Pump.fun BUY: n=3, tylko jeden triplet spełnił rygor wspólnego startu <=10 ms, a operator jawnie dopuścił jeden wspólny payer. Wspólny writable payer mógł wprowadzić między lane account write-lock contention. Nie było podstaw do promowania Direct Jito ani NLN do runtime, zmiany konfiguracji ani zmiany BUY/SELL.

## 2. Stan repo, zakres i źródła prawdy

| Pole | Wartość |
| --- | --- |
| Data runu | 2026-07-23 UTC |
| Lokalny HEAD | 113c5aea19f4da873d21d0c513c9727a886cd270 |
| Zweryfikowany origin/main | a12ef9cfb7199d44841cde27be2ecd8af13e2f3f |
| Lokalna gałąź | agent/rug-scalp-v2-prospective-shadow-20260721 |
| Harness | ghost-launcher/examples/helius_sender_direct_jito_nln_benchmark.rs |
| Ghost runtime | nieuruchamiany |
| Zmiany produkcyjnego LiveTxSender, configu, BUY/SELL | brak |
| Sekrety / private keys / raw TX bytes w raporcie | brak |

Przed implementacją porównano aktualne blob-y origin/main: live_tx_sender.rs (3157bbfec302f1011cda248649763ee1f0535fa0) i jito_client.rs (74c98eeb4897ce092e36629172eb0d39e54d6b77) są identyczne z lokalnym checkoutem. Lokalny trigger/component.rs różni się od origin/main, dlatego mapa aktywnego BUY/SELL wskazuje dokładnie origin/main.

### Rzeczywista aktywna ścieżka Helius Sender

| Wymagany element | Dowód origin/main | Zweryfikowany fakt |
| --- | --- | --- |
| Tworzenie LiveTxSender | ghost-launcher/src/main.rs:630-667 | Przy wymaganym live senderze powstaje pojedynczy Arc<LiveTxSender> z sender endpoint, priority-fee RPC i Yellowstone. |
| Resolved Sender endpoint | ghost-launcher/src/components/live_tx_sender.rs:24-25, 1467-1473 | Domyślnie http://fra-sender.helius-rpc.com/fast; tylko niepusty GHOST_HELIUS_SENDER_ENDPOINT go nadpisuje. W tym runie override nie było. |
| Persistentny Helius HTTP client | live_tx_sender.rs:519-534 | LiveTxSender::new buduje jeden reqwest::Client; harness utrzymał jeden obiekt przez cały run. |
| send_transaction | live_tx_sender.rs:957-1029 | Prawdziwa produkcyjna metoda serializuje podpisany VersionedTransaction i wywołuje standardowe JSON-RPC sendTransaction. |
| skipPreflight / maxRetries | live_tx_sender.rs:975-987, szczególnie 981-985 | Dokładnie skipPreflight=true, maxRetries=0, encoding base64. |
| Wybór konta tipowego | Lista live_tx_sender.rs:44-55; wybór 1483-1488 | Helius wybiera deterministycznie jedno z 10 kont przez blake3(seed_material). Harness wywołał sender.select_tip_account(...), nie skopiowaną listę. |
| Domyślny BUY tip | live_tx_sender.rs:27, 1506-1512 | Baseline to 1 000 000 lamportów. Benchmark użył dokładnie 1 000 000. |
| Priority fee | live_tx_sender.rs:28; aktywne pobieranie/estimate 728-823 | Fallback to 25 000 µlamportów/CU. Benchmark wymusił równy kontrakt 25 000 µlamportów/CU dla wszystkich lane; nie wywoływał dynamicznego estimatora. |
| confirm_submission | live_tx_sender.rs:1032-1151 | Po ACK Helius harness równolegle wywołał dokładnie tę produkcyjną metodę. |
| Commitment production confirmation | live_tx_sender.rs:1050-1074 | transactions_status przy CommitmentLevel::Confirmed. |
| Świeże połączenie confirmation | live_tx_sender.rs:1076-1103 | Metoda buduje i łączy nowy klient Yellowstone z tcp_nodelay, connect timeout 5 s i stream timeout 15 s. |
| Aktualne live BUY | ghost-launcher/src/components/trigger/component.rs:3494-3506, 3659-3702, 3724-3740 | BUY używa send_transaction, wymusza zgodność podpisu lokalnego i zwróconego, potem uruchamia confirmation. |
| Aktualne live SELL | ghost-launcher/src/components/post_buy_runtime.rs:5422-5454, 5476-5485 | SELL używa tej samej metody, sprawdza signature i uruchamia confirmation. |

W konstruktorze benchmarku pole priority_fee_rpc_url zostało zasilone istniejącą zmienną środowiskową potrzebną do utworzenia LiveTxSender, lecz w zmierzonych metodach send_transaction i confirm_submission nie jest odczytywane. Nie jest to więc pomiar dynamicznej production estimation priority fee; stała, równa cena CU jest celowym kontraktem porównania transportu/inclusion.

## 3. Klasyfikacja lane

| Lane | Stan względem runtime Ghosta | Faktycznie użyty kontrakt | Czego nie twierdzimy |
| --- | --- | --- | --- |
| HELIUS_SENDER | **Aktywna** ścieżka live BUY/SELL | Jeden persistentny LiveTxSender; send_transaction do Frankfurt /fast; po ACK confirm_submission | Że timestamp processed jest timestampem wykonania validatora. |
| DIRECT_JITO_GRPC | Zachowany transport trigger::JitoClient, nie aktualny primary BUY/SELL | Publiczne trigger::JitoClient::submit_bundle(vec![tx]); SearcherService/SendBundle; świeży TLS/gRPC channel na submit | Że wrapper zwraca serwerowy echo-signature lub udostępnia Jito UUID. |
| NLN_SENDBUNDLE | Izolowany benchmark, nie runtime | Persistentny reqwest::Client; https://rpc.nln.clr3.org, x-api-key, sendBundle | Że returned bundle_id sam oznacza landing lub bundle-only/revert protection. |

trigger::JitoClient buduje Endpoint, TLS i channel per call (off-chain/components/trigger/src/jito_client.rs:790-857), a publiczne submit_bundle używa istniejącego failoveru przed ACK (873-956, 1794-1849). Harness użył go as-is. Publiczna metoda otrzymuje UUID wewnętrznie, ale zwraca pierwszą lokalną signature; dlatego Direct Jito ma bundle_id_or_none=null, zamiast ręcznie rekonstruowanego klienta dla samego UUID.

## 4. Metodologia i kontrola bezpieczeństwa

### Kontrakt transakcji

Każdy triplet składał się z trzech osobno podpisanych VersionedTransaction:

- ComputeBudget::set_compute_unit_limit(50_000);
- ComputeBudget::set_compute_unit_price(25_000) µlamportów/CU;
- równodługościowe Memo GHOST_BENCH_20260723:<lane>:Tnn;
- inline system_transfer tipa 1 000 000 lamportów;
- jeden wspólny świeży blockhash w ramach tripletu;
- osobne signature i właściwe dla lane, różne publiczne konta tipowe.

Operator jawnie zastąpił pierwotny wymóg trzech payerów jednym istniejącym testowym portfelem (fingerprint 9MCk…vbaw). Nie utworzono ani nie finansowano żadnego portfela. Jest to najsilniejsze ograniczenie interpretacyjne runu: trzy transakcje mogły konkurować o ten sam writable account. Nie użyto wspólnej signature i żadna transakcja nie kupowała tokena ani nie dotykała Pump.fun.

### Stage 0 bez on-chain submita

| Kontrola | Wynik |
| --- | --- |
| Saldo payera przed Stage 1 | 15 079 200 lamportów |
| Helius /ping (oddzielny client, nie /fast warm-up) | HTTP 200 |
| Direct Jito getTipAccounts | 8 kont |
| NLN getTipAccounts | 8 kont |
| Wybrane tip accounts Helius / Direct / NLN | różne |
| Pusty NLN sendBundle | odrzucony walidacyjnie bez transakcji |
| Persistentny Yellowstone processed observer | uzbrojony przed submitami, https://grpc.nln.clr3.org:443 |
| Symulacja Helius / Direct / NLN | 25 470 / 25 470 / 25 470 CU, err=null |
| On-chain submit w Stage 0 | **nie wykonano** |

Pierwsza próba pre-submit w trybie --execute została poprawnie zatrzymana przed barierą: trzeci simulateTransaction dostał od NLN -32005 rate limit. Nie wykonano w niej żadnego submita. Dopiero potem harness dostał 650 ms odstępu między trzema obowiązkowymi symulacjami. Ten odstęp jest przed submit_started, nie jest retry transakcji, nie ociepla /fast i nie zmienia polityki żadnego lane.

### Hard cap i reguły stopu

| Składnik | Lamporty |
| --- | ---: |
| Base fee, 1 podpis | 5 000 |
| Priority fee upper bound | 1 250 |
| Tip | 1 000 000 |
| Hard max / transakcję | 1 006 250 |
| Hard max / 9 transakcji | 9 056 250 = 0,00905625 SOL |

Nie było application-level resubmitu po ACK. Direct Jito zachował jedynie już istniejący failover przed ACK. Po pierwszym tripletcie harness wymagał: 3 x finalized, brak on-chain error, odczyt fee i dokładną zgodność spadku salda z sumą kosztów. Stage 1 przeszedł te warunki, więc uruchomiono Stage 2.

### Semantyka czasu

Główny observer był jeden, persistentny i zarejestrowany dla lokalnych signatures przed wspólną barierą. submit → processed oznacza **czas od lokalnego submit_started do pierwszego matching update processed otrzymanego przez ten Yellowstone**. Nie oznacza execution time validatora ani prawdziwego czasu landingu. RPC polling i status bundle służyły wyłącznie reconciliation.

confirm_submission Helius był uruchamiany po ACK równolegle z reconciliation. Metoda sama otwiera świeże Yellowstone connection. Jej API nie odsłania granicy connect ukończony vs subscribe/confirmed update; dlatego fresh_connection_ms jest celowo null, a nie wymyśloną estymacją. Raportuje się czas całej prawdziwej metody production confirmation.

## 5. Wyniki surowe

Legenda: P oznacza triplet spełniający start gap <=10 ms i kwalifikujący się do głównego paired comparison; D oznacza wynik diagnostyczny (gap >10 ms). Wszystkie czasy są w ms.

| T | Klasa | Lane | start gap | ACK | submit → processed | ACK → processed | submit slot | processed / landed slot | Δ slot | production confirmation | Finalized | fee / total cost |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | --- |
| 1 | P | Helius | 6,069 | 20,780 | 210,085 | 189,306 | 434671237 | 434671239 / 434671239 | 2 | 550,516; CONFIRMED; slot 434671239 | tak | 6 250 / 1 006 250 |
| 1 | P | Direct Jito | 6,069 | 32,165 | 711,035 | 678,870 | 434671237 | 434671240 / 434671240 | 3 | n/a | tak | 6 250 / 1 006 250 |
| 1 | P | NLN | 6,069 | 8,375 | 715,237 | 706,862 | 434671237 | 434671240 / 434671240 | 3 | n/a | tak | 6 250 / 1 006 250 |
| 2 | D | Helius | 13,082 | 26,460 | 46,651 | 20,191 | 434671275 | 434671276 / 434671276 | 1 | 149,832; CONFIRMED; slot 434671276 | tak | 6 250 / 1 006 250 |
| 2 | D | Direct Jito | 13,082 | 49,602 | 147,334 | 97,732 | 434671275 | 434671277 / 434671277 | 2 | n/a | tak | 6 250 / 1 006 250 |
| 2 | D | NLN | 13,082 | 14,624 | 147,874 | 133,250 | 434671275 | 434671277 / 434671277 | 2 | n/a | tak | 6 250 / 1 006 250 |
| 3 | D | Helius | 13,377 | 20,273 | 160,521 | 140,248 | 434671312 | 434671314 / 434671314 | 2 | 593,795; CONFIRMED; slot 434671314 | tak | 6 250 / 1 006 250 |
| 3 | D | Direct Jito | 13,377 | 39,916 | 273,385 | 233,469 | 434671312 | 434671314 / 434671314 | 2 | n/a | tak | 6 250 / 1 006 250 |
| 3 | D | NLN | 13,377 | 13,358 | 210,635 | 197,278 | 434671312 | 434671314 / 434671314 | 2 | n/a | tak | 6 250 / 1 006 250 |

W Helius wszystkie trzy provider_returned_signature były identyczne z lokalną signature. Direct trigger::JitoClient zwrócił identyczną lokalną signature po wewnętrznym ACK. Każdy NLN submit zwrócił różny bundle_id, a getBundleStatuses później podało confirmation_status=confirmed, err={Ok:null}, właściwy slot i zawartą signature. On-chain getSignatureStatuses niezależnie podało finalized i err=null dla 9/9.

## 6. Statystyka n=3

Nie podano p95/p99. Statystyka obejmuje trzy faktyczne próby danego lane, ale T2 i T3 są diagnostyczne dla paired comparison.

| Lane | Metryka | min | mean | median | max | range |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Helius | ACK ms | 20,273 | 22,504 | 20,780 | 26,460 | 6,187 |
| Direct Jito | ACK ms | 32,165 | 40,561 | 39,916 | 49,602 | 17,437 |
| NLN | ACK ms | 8,375 | 12,119 | 13,358 | 14,624 | 6,249 |
| Helius | submit → processed ms | 46,651 | 139,086 | 160,521 | 210,085 | 163,434 |
| Direct Jito | submit → processed ms | 147,334 | 377,251 | 273,385 | 711,035 | 563,701 |
| NLN | submit → processed ms | 147,874 | 357,916 | 210,635 | 715,237 | 567,363 |
| Helius | ACK → processed ms | 20,191 | 116,581 | 140,248 | 189,306 | 169,115 |
| Direct Jito | ACK → processed ms | 97,732 | 336,691 | 233,469 | 678,870 | 581,138 |
| NLN | ACK → processed ms | 133,250 | 345,797 | 197,278 | 706,862 | 573,612 |
| Helius | Δ slot | 1 | 1,667 | 2 | 2 | 1 |
| Direct Jito | Δ slot | 2 | 2,333 | 2 | 3 | 1 |
| NLN | Δ slot | 2 | 2,333 | 2 | 3 | 1 |

### Cold vs reused Helius Sender

Jedna instancja LiveTxSender powstała przed runem. Nie wykonano requestu /fast dla prewarm. T1 to pierwsze użycie persistentnego clienta (cold/first-use), T2–T3 to reused-client:

| Helius try | Klasa | ACK ms | submit → processed ms | confirm_submission ms |
| ---: | --- | ---: | ---: | ---: |
| 1 | cold/first-use | 20,780 | 210,085 | 550,516 |
| 2 | reused-client | 26,460 | 46,651 | 149,832 |
| 3 | reused-client | 20,273 | 160,521 | 593,795 |

Nie należy odczytywać tej trójki jako stabilnej charakterystyki warm/cold; pokazuje tylko rzeczywiste pierwsze i kolejne użycia zgodne z kontraktem produkcyjnym.

## 7. Paired differences

Wartość ujemna oznacza, że pierwszy lane w nazwie był szybszy lub miał mniejszy dystans slotowy. Tylko T1 jest P; T2–T3 pozostają wymaganymi surowymi porównaniami diagnostycznymi i **nie są podstawą paired winnera**.

| T | Klasa | Różnica | ACK ms | submit → processed ms | ACK → processed ms | Δ slot |
| ---: | --- | --- | ---: | ---: | ---: | ---: |
| 1 | P | Helius − Direct Jito | -11,385 | -500,950 | -489,564 | -1 |
| 1 | P | Helius − NLN | 12,405 | -505,152 | -517,557 | -1 |
| 1 | P | NLN − Direct Jito | -23,790 | 4,202 | 27,992 | 0 |
| 2 | D | Helius − Direct Jito | -23,141 | -100,683 | -77,542 | -1 |
| 2 | D | Helius − NLN | 11,837 | -101,223 | -113,060 | -1 |
| 2 | D | NLN − Direct Jito | -34,978 | 0,540 | 35,518 | 0 |
| 3 | D | Helius − Direct Jito | -19,643 | -112,865 | -93,222 | 0 |
| 3 | D | Helius − NLN | 6,916 | -50,115 | -57,030 | 0 |
| 3 | D | NLN − Direct Jito | -26,558 | -62,750 | -36,191 | 0 |

## 8. Production confirm_submission Helius

| Try | ACK → start confirmation ms | Czas całej metody ms | Outcome | Slot zwrócony przez production confirmation | Niezależny on-chain wynik |
| ---: | ---: | ---: | --- | ---: | --- |
| 1 | 0,097 | 550,516 | CONFIRMED | 434671239 | finalized, ten sam slot |
| 2 | 0,852 | 149,832 | CONFIRMED | 434671276 | finalized, ten sam slot |
| 3 | 0,098 | 593,795 | CONFIRMED | 434671314 | finalized, ten sam slot |

Wynik to 3/3 pozytywne rzeczywiste LiveTxSender::confirm_submission. Nie ma timeoutu ani transport error. Nie jest to wynik bezpośrednio porównywalny z Direct/NLN: te lane celowo nie dostały Jito-status/bundle API jako zastępczego production confirmation. Timeout tej metody nie byłby zaklasyfikowany jako brak landingu — końcowe rozstrzygnięcie zawsze pozostaje on-chain.

## 9. Landing, finality i saldo

| Kontrola | Wynik |
| --- | --- |
| Helius landed / finalized | 3 / 3 |
| Direct Jito landed / finalized | 3 / 3 |
| NLN landed / finalized | 3 / 3 |
| On-chain err | 9 x null |
| Stage 1 saldo | 15 079 200 → 12 060 450, delta 3 018 750 = dokładnie 3 x 1 006 250 |
| Cały run saldo | 15 079 200 → 6 022 950, delta 9 056 250 = dokładnie 9 x 1 006 250 |
| Koszt lane | 3 x 1 006 250 = 3 018 750 lamportów |
| Przekroczenie hard capu | nie; koszt finalny = hard cap |

Actual fee wynosiło 6 250 lamportów dla każdej transakcji: 5 000 base fee + 1 250 maksymalnej opłaty priorytetowej dla zadanego CU price. Z tipem 1 000 000 daje to 1 006 250 lamportów na transakcję. Nie była potrzebna eskalacja tipu do 0,003–0,004 SOL.

## 10. Werdykty per wymiar

| Werdykt | Wynik | Uzasadnienie |
| --- | --- | --- |
| ACK WINNER | **NLN** | NLN był najszybszy w 3/3 ACK (mean 12,119 ms), ale ACK nie jest inclusion. |
| PROCESSED-TO-GHOST WINNER | **HELIUS** | Helius miał najmniejsze submit → processed we wszystkich 3 raw próbach i wygrał jedyny kwalifikowany T1. To receipt w konkretnym Yellowstone, nie validator execution time. |
| SLOT-DISTANCE WINNER | **HELIUS** | Mean Δ slot 1,667 vs 2,333 dla Direct i NLN; T1 wygrał o jeden slot. Różnica 0–1 slotu nie dowodzi stałej przewagi. |
| LANDING RELIABILITY WINNER | **TIE** | Wszystkie lane 3/3 finalized, err=null; n=3 nie jest SLO. |
| PRODUCTION CONFIRMATION WINNER | **INCONCLUSIVE** | Helius 3/3 potwierdził własnym aktualnym kontraktem, lecz lane B/C nie są równoważnie mierzone przez ten kontrakt. |
| ATTRIBUTION WINNER | **NLN** | NLN oddał bundle_id, status bundle i został niezależnie potwierdzony on-chain. Direct exact public API ukrywa UUID. |
| COST WINNER | **TIE** | Każda z 9 transakcji kosztowała 1 006 250 lamportów. |
| OVERALL GHOST EXECUTION VERDICT | **HELIUS PRIMARY, NLN CANDIDATE REDUNDANT LANE** | Helius zachowuje primary dzięki rzeczywistemu kontraktowi BUY/SELL, szybszemu processed w tym runie i pozytywnemu production confirmation. NLN jest kandydatem na przyszły redundant lane dzięki ACK i attribution, nie na podstawie ACK jako primary replacement. |

## 11. Dowiedzione fakty

1. Aktualny Helius Sender Ghosta, Direct trigger::JitoClient i faktyczne NLN sendBundle zostały uruchomione w jednym krótkim oknie z ekonomicznie równymi Memo probe transactions.
2. Helius lane wywołał bez modyfikacji prawdziwe LiveTxSender::send_transaction i po ACK prawdziwe confirm_submission; wszystkie trzy returned signatures były identyczne z lokalnymi.
3. NLN przyjmuje sendBundle z x-api-key, zwraca bundle_id i udostępnia provider status rozstrzygalny niezależnie od on-chain result.
4. Każdy z dziewięciu lokalnie podpisanych payloadów przeszedł obowiązkową symulację przed własnym submitem; każda z dziewięciu transakcji została finalized bez błędu, a saldo i koszty się zgadzają.
5. W tym konkretnym observerze Helius dotarł do pierwszego matching processed wcześniej niż pozostałe lane w każdym z trzech raw tripletów.

## 12. Niedowiedzione hipotezy

- że Helius jest stale szybszy niż Direct/NLN dla prawdziwego Pump.fun BUY;
- że NLN sendBundle zapewnia bundleOnly, revert protection lub wyłączność order flow względem normalnego leader forwarding;
- że Direct Jito nie wykonał internal failoveru przed ACK — publiczne API nie odsłania liczby prób/endpointu końcowego dla submit_bundle;
- że wynik 3/3 jest produkcyjnym SLO, landed-rate lub metryką leader schedule;
- że różnica jednego slotu będzie trwała w innym momencie, regionie lub pod launch contention;
- że Memo+tip probe zachowa się identycznie jak złożony Pump.fun BUY.

## 13. Ograniczenia

- n=3; nie ma p95/p99 ani wniosku o trwałej przewadze;
- tylko T1 spełnił limit wspólnego startu <=10 ms; T2 i T3 są transparentnie oznaczone jako diagnostyczne;
- pojedynczy payer, zaakceptowany przez operatora, tworzy potencjalny write-lock i różni się od pierwotnej izolacji trzech walletów;
- primary observer jest jednym providerem Yellowstone; nie odejmujemy minimum z wielu observerów;
- Direct Jito publiczna metoda zwraca lokalną signature po ACK, nie API UUID;
- Helius production confirmation nie eksponuje osobno czasu zestawienia nowego połączenia od subskrypcji/await update;
- obserwacja processed nie jest timestampem validator execution;
- nie wykonano ani Pump.fun BUY, ani realnego order flow / MEV testu.

## 14. Decyzja dla Ghosta

Odpowiedź na pytanie końcowe brzmi: **aktywna ścieżka Helius Sendera pozostaje najlepszą podstawową ścieżką egzekucji Ghosta w świetle tego runu.** NLN sendBundle dostarcza wartościowy, zweryfikowany candidate redundant lane i ma lepszą attribution niż exact public Direct Jito wrapper. Nie ma jednak wystarczająco mocnego dowodu, aby na podstawie tego n=3 zmienić Helius primary lane; szczególnie nie na podstawie najszybszego ACK NLN.

Ewentualny kolejny krok wymaga odrębnej decyzji o większym burn-inie z odseparowanymi payerami oraz ochrony przed shared-payer contention. Dopiero wtedy można rozważyć runtime integration lub primary-lane change. Ten benchmark sam niczego do runtime nie promuje.

## 15. Walidacja

Po zmianie izolowanego example wykonano:

    cargo fmt --all -- --check
    cargo check -p ghost-launcher --example helius_sender_direct_jito_nln_benchmark
    cargo test -p ghost-launcher --example helius_sender_direct_jito_nln_benchmark
    cargo build -p ghost-launcher --example helius_sender_direct_jito_nln_benchmark
    git diff --check

Wszystkie komendy kończą się powodzeniem. Workspace wypisuje istniejące ostrzeżenia z ghost-core, seer, trigger, ghost-brain i ghost-launcher; nie wynikają one z benchmarku. Run live wykonał 9 transakcji i nie logował secretów, private key ani raw serializacji.

