# Kontrolowany benchmark bundle: Direct Jito gRPC vs NLN `sendBundle` — 2026-07-23

## Werdykt

Benchmark został wykonany i poprawnie rozliczony: **3/3 transakcje Direct Jito oraz 3/3 transakcje NLN `sendBundle` osiągnęły `finalized`, z `err=null`**. Każdy lane zwrócił własny `bundle_id`; dla każdej sygnatury istnieje niezależny dowód `processed` z Yellowstone oraz późniejszy dowód statusu bundle i transakcji on-chain.

W tej małej, kontrolowanej próbce NLN było wyraźnie szybsze do ACK (mediana **13,733 ms** vs **30,674 ms**), a obserwowane `submit → Yellowstone processed` było niższe o medianę **22,565 ms** (271,671 ms vs 294,236 ms). To nie wystarcza jednak, aby uznać NLN za bezwarunkowy zamiennik aktywnej ścieżki Ghosta: aktualny live BUY/SELL Ghosta jest Sender-only, a lane A jest zgodnym z `trigger::JitoClient` zachowanym transportem Direct Jito, nie aktywną ścieżką BUY/SELL.

**Ogólny werdykt dla decyzji produkcyjnej Ghosta: `INCONCLUSIVE`.**

**Werdykt dla dokładnie zmierzonych, izolowanych transportów:** NLN `sendBundle` ma mocny wynik ACK i słabszą, lecz dodatnią wskazówkę dla `processed`; oba transporty mają 100% landing/finality w tej serii. Przed ewentualnym zastąpieniem sendera potrzebny jest osobny test z realną aktywną ścieżką Ghosta oraz niezależnymi payerami, aby wyeliminować contention wspólnego konta.

## Zakres i uczciwa klasyfikacja lane

| Lane | Rzeczywiście użyty transport | Czy to aktywna ścieżka live BUY/SELL Ghosta? | Wynik klasyfikacji |
| --- | --- | --- | --- |
| A — `DIRECT_JITO_GRPC` | `SearcherService/SendBundle` przez świeży kanał TLS gRPC; Frankfurt → Amsterdam → London → Dublin, zgodnie z `off-chain/components/trigger/src/jito_client.rs` | Nie. Testy invariants Ghosta wymagają obecnie Helius Sender + Yellowstone i zakazują legacy Jito bundle dla live BUY/SELL. | Rzetelny benchmark zachowanego transportu `trigger::JitoClient`, **nie** pomiar aktualnej live ścieżki BUY/SELL. |
| B — `NLN_SENDBUNDLE` | Uwierzytelniony HTTP JSON-RPC `sendBundle` do `https://rpc.nln.clr3.org` z `x-api-key` | Nie zmienia runtime Ghosta; jest ręcznie uruchomionym izolowanym probe. | Rzeczywisty NLN `sendBundle`, nie `sendTransaction`, z returned `bundle_id` i status API. |

Direct Jito gRPC jest zgodny z aktualnym kodem `trigger::JitoClient`: tworzy kanał TLS per submit, opcjonalnie dołącza `x-jito-auth`, używa `SearcherService/SendBundle` i tej samej kolejności EU failover. Zostało to celowo zmierzone **as-is**, bez „ocieplenia” kanału. Kod Ghosta potwierdza jednocześnie, że aktywny BUY jest fail-closed na Senderze, więc raport nie nazywa lane A aktywnym BUY senderem.

Oficjalny Jito opisuje `sendBundle` jako API zwracające identyfikator bundle i wymagające późniejszego sprawdzenia statusu — ACK nie jest landingiem. [Jito Low Latency Transaction Send](https://docs.jito.wtf/lowlatencytxnsend/)

## Metoda oraz bramki bezpieczeństwa

Narzędzie: [`off-chain/components/seer/examples/nln_jito_submission_benchmark.rs`](../../off-chain/components/seer/examples/nln_jito_submission_benchmark.rs).

Nie zmieniono kodu produkcyjnego, Gatekeepera, Seera runtime, konfiguracji produkcyjnej, sendera, retry/failover policy ani logów Ghosta. Uruchomiono osobny proces z pojedynczym, persistentnym Yellowstone gRPC observerem filtrowanym payerem, a lokalnie dopuszczającym wyłącznie sześć wcześniej zarejestrowanych signatures. `Instant` jest używany wyłącznie do różnic czasu lokalnego.

### Stage 0 — bez on-chain submitu

Przed testem płatnym:

- zweryfikowano `https://rpc.nln.clr3.org` z `x-api-key` oraz `https://grpc.nln.clr3.org:443`;
- potwierdzono, że NLN przyjmuje prawdziwe JSON-RPC `sendBundle`; pusty bundle został odrzucony walidacyjnie (`bundle has no transactions`) bez submitu;
- pobrano `getTipAccounts`: Direct Jito zwrócił 8 kont, NLN 8 kont; po normalizacji zestawy były identyczne;
- zweryfikowano `getBundleStatuses` oraz `getInflightBundleStatuses` po obu stronach;
- uruchomiono `cargo fmt --all -- --check`, testy example i build;
- wymuszono `--execute`, `--max-pairs 3` i `BENCH_MAX_TOTAL_LAMPORTS=12_100_000`.

Pierwsza bezkosztowa symulacja ujawniła błąd harnessu: limit 20 000 CU kończył się w Memo (`ProgramFailedToComplete`). Nie wysłano wtedy bundle’a. Limit example został skorygowany do 50 000 CU, symulacja obu wcześniej podpisanych payloadów przeszła przed każdą parą, a maksymalny koszt serii obliczono ponownie jako 12 030 300 lamportów. To jest istotne: odrzucony preflight **nie** jest próbą benchmarkową ani kosztem on-chain.

### Wykonanie płatne

- Trzy pary; kolejność: Direct → NLN, NLN → Direct, Direct → NLN.
- Dwie osobne, równoważne transakcje w każdej parze: wspólny świeży blockhash, ten sam payer, CU limit, priority fee i tip account; różni je wyłącznie Memo lane/sequence oraz signature.
- Payloady przygotowano, podpisano i zserializowano przed punktem `submit_started`.
- Krótki, kontrolowany offset 5 ms oraz naprzemienna kolejność; realny gap startów: 11,772 / 6,729 / 11,650 ms — każda para spełnia limit porównywalności ≤25 ms.
- Brak application-level re-submit po ACK. Failover Direct Jito mógł działać tylko przed ACK, tak jak w kompatybilnym kliencie produkcyjnym; nie był potrzebny, wszystkie ACK przyszły z Frankfurtu.
- `processed` mierzono z persistentnego Yellowstone, nie pollingiem. Polling służył wyłącznie bundle status / confirmation / finality.

Nie ma dostępnego, wiarygodnego timestampu wykonania pojedynczej instrukcji po stronie validatora. Dlatego `on-chain T0 → Ghost` jest **not directly measurable with available payload**. Główna metryka tego raportu to uczciwie nazwana: **lokalny `submit_started → pierwszy matching Yellowstone processed`**.

## Koszt

| Pozycja | Wartość |
| --- | ---: |
| Liczba par / transakcji | 3 / 6 |
| Tip na transakcję | 2 000 000 lamportów (0,002 SOL) |
| CU limit / CU price | 50 000 / 1 000 µlamportów per CU |
| Priority fee upper bound | 50 lamportów |
| Faktyczna fee na transakcję | 5 050 lamportów |
| Koszt jednej transakcji | 2 005 050 lamportów |
| Direct Jito: 3 transakcje | 6 015 150 lamportów |
| NLN: 3 transakcje | 6 015 150 lamportów |
| Łączny koszt testu | **12 030 300 lamportów (0,0120303 SOL)** |
| Saldo testowego payera | 27 109 500 → 15 079 200 lamportów |

Różnica salda wyniosła dokładnie 12 030 300 lamportów, czyli sześć razy tip plus on-chain fee. Nie wystąpił retry ani koszt wykraczający poza jawny limit.

### Ważne odstępstwo od najnowszego wymagania „minimalnego tipu”

Ten live run użył 0,002 SOL per transakcję, ponieważ użytkownik wcześniej wprost zatwierdził ten hard cap dla testu i taki tip był już skonfigurowany po nieudanej, bezkosztowej walidacji. Nie była to automatyczna eskalacja. Jest to jednak wyższa opłata niż minimalny tip preferowany przez późniejszą specyfikację. Nie podważa równoważności obu lane ani pomiaru czasu, ale oznacza, że raport **nie jest minimalno-kosztowym profilem zachowania przy minimum tipa**. Po wykonaniu pełnych trzech par nie wykonano dodatkowej serii tylko po to, aby zmienić tip.

## Surowe wyniki

Wszystkie milisekundy są lokalnymi różnicami `Instant`; `landed` pochodzi z kontrolowanego status polling i nie jest przedstawiane jako precyzyjny timestamp wykonania validatora. Wartości `finalized seen` dla pair 2 były poza lokalnym oknem runu, ale zostały później niezależnie potwierdzone jako `finalized` przez signature status, `getTransaction` i status bundle; nie dopisano im sztucznej wartości czasu finality.

| Pair | Lane | Kolejność | ACK ms | Submit → Yellowstone processed ms | ACK → Yellowstone processed ms | Submit slot | Landed slot | Δ slot | Bundle ID | Stan końcowy | Fee + tip |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: |
| 1 | Direct Jito gRPC | 1 | 28,084 | 294,236 | 266,152 | 434622504 | 434622505 | 1 | `43e7f821…ccc23ca` | FINALIZED | 2 005 050 |
| 1 | NLN `sendBundle` | 2 | 16,801 | 271,671 | 254,870 | 434622504 | 434622505 | 1 | `d8d36b47…0e0b8` | FINALIZED | 2 005 050 |
| 2 | NLN `sendBundle` | 1 | 13,504 | 307,510 | 294,006 | 434622537 | 434622539 | 2 | `7b180f5d…4f24fb` | FINALIZED* | 2 005 050 |
| 2 | Direct Jito gRPC | 2 | 31,340 | 300,856 | 269,516 | 434622537 | 434622539 | 2 | `10dc8779…c98053` | FINALIZED* | 2 005 050 |
| 3 | Direct Jito gRPC | 1 | 30,674 | 289,124 | 258,449 | 434622540 | 434622542 | 2 | `ac0ec1b4…3ea82a` | FINALIZED | 2 005 050 |
| 3 | NLN `sendBundle` | 2 | 13,733 | 234,556 | 220,824 | 434622540 | 434622541 | 1 | `0d282d5d…ff4e0f` | FINALIZED | 2 005 050 |

\* Finality potwierdzona post-run, bez wiarygodnej wartości `submit → finalized seen` z lokalnego okna.

### Identyfikatory i niezależne dowody

| Pair | Lane | Signature | Pełny bundle ID | Dowód końcowy |
| ---: | --- | --- | --- | --- |
| 1 | Direct | `5MfMyGpEHrCz5nmpzU7Y7ftnqJsRWXMqD9ymhX4ECSeRPxgEpHK4DrUr2gkEkcypkVwwEVEHt5C5dNhpXaSvww97` | `43e7f821a5db1ac7414de642d661c45202b624d385bd0cfb5de059986ccc23ca` | Jito bundle status `finalized`, Solana `finalized`, `getTransaction err=null` |
| 1 | NLN | `43fBbLQpAfURB3duZQGM4RAwnjuwpVvXfxoy3puCJCojnJ1QMfNnrCqbuBCVgHuegqMVP2CRYk4kwXLPrg7LJvSR` | `d8d36b472490155488473afe62b7b7d018c3370ad8c32d703c6d425d5ef0e0b8` | NLN bundle status `finalized`, Solana `finalized`, `getTransaction err=null` |
| 2 | Direct | `4zeagaymPEHHWooPJAXVmKLd1789mgBiYXpqAkCyLWFVCABaiM8uN7xEMSASJAwsUiZsc8UhKvVnGJGvjYfryAGH` | `10dc8779b076a340ba87b14af65235e8c9e31564fec6318821b7865531c98053` | Jito bundle status `finalized`, Solana `finalized`, `getTransaction err=null` |
| 2 | NLN | `5kmWCd7HqeYKMpbkhCCN72AZW3rp2K1rc18aUjBairwg21Ym3iJbHEgTS8jCof45miv4B6PBaD4dxhdN6mTKM9Dx` | `7b180f5d3f48dc57103abe0ecd2f9be12ccb16cd87c0a670aee7d069ca4f24fb` | NLN bundle status `finalized`, Solana `finalized`, `getTransaction err=null` |
| 3 | Direct | `4t6PzbW1Y9vdUvYSEUSiWsizGkzbcS9BNWwjeiFfmVw9eTyUt9uXED7mUBd5xoQ62n8PzV2giuXM3Lvu2pNaQooX` | `ac0ec1b4c76bb5797941a1539def8ce908e9d06887724c0b601221c4553ea82a` | Jito bundle status `finalized`, Solana `finalized`, `getTransaction err=null` |
| 3 | NLN | `qY6VdwzT53C7fvnHyvk5SogXnYkJEuH8Z6ttbkxTVQjUyXW2UoNQwH31b4vX1BMBCwM9kpsSqeRjYMPjFwNxhrX` | `0d282d5dffa514ae228580f496edf8ee897c41ffbb102fe8950e0994bbff4e0f` | NLN bundle status `finalized`, Solana `finalized`, `getTransaction err=null` |

## Podsumowanie lane — n=3

Bez p95/p99. `Finalized latency` ma tylko n=2, ponieważ dla jednej pary w każdym lane wykryto finality dopiero po zamknięciu lokalnego okna obserwacji.

| Metryka | Direct Jito gRPC: min / mean / median / max / range | NLN `sendBundle`: min / mean / median / max / range |
| --- | --- | --- |
| Provider ACK (ms) | 28,084 / 30,033 / 30,674 / 31,340 / 3,256 | **13,504 / 14,679 / 13,733 / 16,801 / 3,297** |
| Submit → Yellowstone processed (ms) | 289,124 / 294,739 / 294,236 / 300,856 / 11,733 | **234,556 / 271,246 / 271,671 / 307,510 / 72,953** |
| ACK → Yellowstone processed (ms) | 258,449 / 264,706 / 266,152 / 269,516 / 11,067 | **220,824 / 256,566 / 254,870 / 294,006 / 73,182** |
| Submit → first landed seen (ms) | 304,356 / 585,165 / 705,385 / 745,755 / 441,399 | **269,042 / 438,039 / 292,588 / 752,488 / 483,446** |
| Submit → finalized seen (n=2; ms) | 14 000,477 / 14 686,730 / 14 686,730 / 15 372,982 / 1 372,504 | **13 988,711 / 14 462,673 / 14 462,673 / 14 936,635 / 947,924** |
| ACK / Yellowstone processed / finalized | 3/3 / 3/3 / 3/3 | 3/3 / 3/3 / 3/3 |
| Δ landed slot: raw / median | 1, 2, 2 / 2 | **1, 2, 1 / 1** |

Pary są ważniejsze od samej średniej: względna różnica `NLN − Direct` dla `submit → processed` wyniosła −22,565 ms, +6,654 ms i −54,567 ms. Sugeruje przewagę NLN w dwóch z trzech par, ale nie jest stabilnym dowodem przewagi infrastrukturalnej przy n=3.

## Odpowiedzi wymagane przez benchmark

| Pytanie | Odpowiedź |
| --- | --- |
| Która trasa szybciej zwraca ACK? | **NLN `sendBundle`**. Wszystkie trzy wyniki NLN (13,504–16,801 ms) są niższe niż wszystkie Direct Jito (28,084–31,340 ms). Obejmuje to celowo mierzoną różnicę: fresh gRPC Direct vs warmed persistent HTTPS NLN. |
| Która szybciej pojawia się na `processed` w Yellowstone? | **Słaba obserwowana przewaga NLN**: mediana 271,671 vs 294,236 ms, średnia 271,246 vs 294,739 ms. Jeden z trzech pairów był jednak wolniejszy dla NLN, a zmienność NLN jest większa. |
| Która ma mniejszy slot distance? | **NLN w próbce**: median 1 slot vs 2 sloty; różnica jest mała i leader-schedule dependent. |
| Czy NLN daje bundle ID i status bundle? | **Tak.** Każdy `sendBundle` zwrócił bundle ID; NLN `getBundleStatuses` zwrócił slot, transaction list, `confirmation_status=finalized` i `err={Ok:null}`. |
| Czy oba lane osiągnęły finality? | **Tak, 3/3** w każdym lane, po niezależnej post-run reconciliation. |
| Która ma lepszą atrybucję bundle? | **Remis dla śladu technicznego**: oba mają bundle ID + status + signature + transaction. **Direct Jito dla udokumentowanej semantyki**: test nie znalazł publicznie udokumentowanego odpowiednika NLN `bundleOnly` / revert protection ani dowodu, że NLN używa wyłącznie Jito zamiast dodatkowego routingu. |
| Która jest tańsza? | **Remis** — identyczny payer, CU price, tip i finalna fee: 2 005 050 lamportów na transakcję. |

### Werdykty

- **ACK WINNER:** NLN `sendBundle`.
- **PROCESSED-TO-GHOST WINNER:** ostrożnie NLN `sendBundle` w tej próbce; nie jest to rozstrzygnięcie produkcyjne.
- **LANDING RELIABILITY WINNER:** remis, 3/3 finalized dla obu.
- **BUNDLE ATTRIBUTION WINNER:** remis dla observable IDs/statusów; Direct Jito wygrywa tylko w zakresie publicznie opisanej semantyki Jito.
- **COST WINNER:** remis.
- **OGÓLNY WERDYKT DLA GHOST:** `INCONCLUSIVE`, ponieważ testowany Direct Jito nie jest aktualnym aktywnym BUY/SELL senderem Ghosta, n=3 jest małe, a wspólny payer tworzy contention.

## Czego test dowiódł, a czego nie

### Dowiedzione

1. NLN aktualnie udostępniło działające `sendBundle`, a nie zwykłe `sendTransaction`.
2. NLN zwróciło per-bundle identyfikator i przez własne API statusu pozwoliło zrekoncyliować landing/finality.
3. Direct Jito gRPC i NLN `sendBundle` osiągnęły w tym samym krótkim oknie 100% finality na minimalnej, nieszkodliwej strukturze Memo + ComputeBudget + inline tip.
4. W tym konkretnym hostingu i okresie NLN odbierało ACK szybciej, a obserwacja `processed` w Yellowstone była przeciętnie krótsza.
5. Żadna metryka `processed` nie pochodzi z `getSignatureStatuses`; główna detekcja była gRPC.

### Niedowiedzione / ograniczenia

1. **Brak aktywnej parzystości produkcyjnego sendera Ghosta.** Live BUY/SELL Ghosta korzysta z Sendera, nie legacy Jito bundle. Nie wolno z tego raportu wyciągać wniosku „zastąpić aktywny sender NLN”.
2. **n=3.** Nie ma p95/p99 ani SLO; seria jest reprezentatywna wyłącznie dla krótkiego punktu w czasie i bieżącego leader schedule.
3. **Wspólny payer.** Każda para ma osobne signatures zgodnie ze specyfikacją, ale oba transfery tipa zapisują ten sam payer, więc mogą konkurować o write lock. Naprzemienna kolejność ogranicza systematyczny bias, ale go nie usuwa.
4. **Brak realnego Pump buy oraz launch contention.** To celowo mierzy transport i inclusion probe, nie slippage, account contention programu Pump, ATA ani stale quote.
5. **Brak dokładnego on-chain T0.** Nie istnieje validator-created timestamp dla pojedynczej transakcji w użytym Yellowstone payloadzie; nie podstawiono pod niego pingów ani czasu slotu.
6. **Tip nie jest minimalny.** Wynik może różnić się od zachowania przy minimalnym aktualnym tipie.
7. **Semantyka NLN nie jest pełna.** Zwrócone bundle ID/status dowodzą śledzalnego bundle route, ale nie dowodzą `bundleOnly`, revert protection, wyłączności ścieżki Jito ani braku równoległego forwardingu do leaderów. Strona NLN opisuje bundle endpoints ogólnie, bez kompletnego kontraktu tych właściwości. [NLN RPC Nodes](https://nolimitnodes.com/products/rpc-nodes) · [NLN API introduction](https://nolimitnodes.com/docs/introduction)
8. Jednorazowy ręczny follow-up statusów Jito dostał HTTP 429 po zbyt bliskich zapytaniach historycznych; nie wykonał retry transakcji i nie wpłynął na `processed` ani landing. Kontrolowany harness ograniczał status Jito do co najmniej 1,05 s, a końcową reconciliację NLN wykonano przez NLN status API oraz on-chain status/transaction.

## Artefakty, weryfikacja i następny bezpieczny krok

Weryfikacje lokalne po zmianie harnessu:

```text
cargo fmt --all -- --check                    # PASS
cargo test -q -p seer --example nln_jito_submission_benchmark   # PASS, 3/3
cargo build -q -p seer --example nln_jito_submission_benchmark  # PASS
```

Następny sensowny test, wyłącznie po osobnej decyzji, powinien mierzyć rzeczywistą aktywną ścieżkę Sender Ghosta obok Direct Jito i NLN `sendBundle`, na trzech odseparowanych payerach lub ze świadomie modelowanym contention. Nie należy automatycznie podłączać tego example do runtime ani zmieniać konfiguracji na podstawie n=3.

