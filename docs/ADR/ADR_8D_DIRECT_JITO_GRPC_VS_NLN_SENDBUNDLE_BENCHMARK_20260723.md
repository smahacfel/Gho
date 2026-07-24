# ADR-8D: Kontrolowany benchmark Direct Jito gRPC vs NLN `sendBundle` — 2026-07-23

## Status

Accepted — jednorazowy, kosztowo ograniczony benchmark live został wykonany, zrekoncyliowany i udokumentowany. Nie jest decyzją o zmianie produkcyjnego sendera Ghosta.

## D1. Problem

Dotychczasowa próba porównywała Direct Jito `sendBundle` z NLN `sendTransaction`, a więc nie porównywała równoważnych usług. Potrzebny był mały i mierzalny test rzeczywistego NLN `sendBundle`, który rozdziela:

- provider ACK;
- pierwsze `processed` odebrane przez persistentny Yellowstone observer;
- bundle status, landed slot i on-chain finality;
- dwa niezależne lane z osobnymi signatures, aby nie zamaskować wyniku przez `AlreadyProcessed`.

Jednocześnie wymagano zerowej ingerencji w Ghost runtime, Gatekeeper, konfigurację oraz aktywny sender.

## D2. Decyzja

Zastąpiono wyłącznie ręcznie uruchamiany example benchmarku:

`off-chain/components/seer/examples/nln_jito_submission_benchmark.rs`

Narzędzie:

- wymaga `--execute`, `--max-pairs` i `BENCH_MAX_TOTAL_LAMPORTS` przed jakimkolwiek on-chain submit;
- rozróżnia Direct Jito `SearcherService/SendBundle` gRPC od NLN HTTP JSON-RPC `sendBundle`;
- odzwierciedla TLS, fresh channel per submit i kolejność EU failover z `trigger::JitoClient`;
- używa persistentnego Yellowstone observera jako głównego źródła `processed`;
- wykonuje `simulateTransaction` dla obu gotowych transakcji przed ich rejestracją i submit;
- nie robi application-level re-submit po ACK i nie eskaluje tipu automatycznie;
- zapisuje raw evidence w raporcie, a nie w Ghost runtime.

Dodano raport zbiorczy:

`PLANS/AUDYT/RAPORT_BENCHMARK_DIRECT_JITO_GRPC_VS_NLN_SENDBUNDLE_20260723.md`

## D3. Kontekst

Aktualny `trigger::JitoClient` posiada bezpośredni Jito gRPC transport, ale testy Ghosta wymagają, aby aktywny live BUY/SELL działał przez Sender. Dlatego lane A jest kompatybilnym pomiarem zachowanego Jito transportu, nie twierdzeniem o obecnej ścieżce live BUY/SELL.

Stage 0 potwierdził w praktyce kontrakt NLN: endpoint `https://rpc.nln.clr3.org`, nagłówek `x-api-key`, JSON-RPC `sendBundle`, returned bundle ID oraz `getBundleStatuses`. Pusty bundle został odrzucony bez wysyłki. Direct i NLN zwróciły ten sam zestaw ośmiu Jito tip accounts.

Pierwszy dry-run ujawnił, że 20 000 CU nie wystarcza dla podpisanego Memo oraz transferu tipa. Przed jakimkolwiek submitem podniesiono limit example do 50 000 CU; symulacje potem przeszły.

## D4. Dowody

Run 2026-07-23, trzy pary, naprzemienna kolejność lane i gap startu ≤25 ms:

| Lane | ACK median | Submit → Yellowstone processed median | Finality | Koszt lane |
| --- | ---: | ---: | ---: | ---: |
| Direct Jito gRPC | 30,674 ms | 294,236 ms | 3/3 | 6 015 150 lamportów |
| NLN `sendBundle` | 13,733 ms | 271,671 ms | 3/3 | 6 015 150 lamportów |

Każda z sześciu transakcji ma oddzielne signature i bundle ID. Późniejsza reconciliation potwierdziła dla wszystkich `finalized`, `err=null`, landed slot oraz fee 5 050 lamportów. Saldo testowego payera zmieniło się dokładnie z 27 109 500 do 15 079 200 lamportów, czyli o limitowo przewidywane 12 030 300 lamportów.

Pełne raw wyniki, signatures, bundle IDs, slot distances i ograniczenia znajdują się w raporcie. Nie przedstawiono p95/p99 dla n=3.

Weryfikacja kodu benchmarku:

- `cargo fmt --all -- --check` — PASS;
- `cargo test -q -p seer --example nln_jito_submission_benchmark` — PASS, 3/3;
- `cargo build -q -p seer --example nln_jito_submission_benchmark` — PASS.

## D5. Odrzucone alternatywy

### NLN `sendTransaction` zamiast `sendBundle`

Odrzucono. To porównuje inny produkt i nie daje wymaganej bundle attribution.

### Wspólna signature dla obu lane

Odrzucono. Pierwszy landing zamienia drugi wynik w `AlreadyProcessed` i uniemożliwia przypisanie czasu do providera.

### Polling RPC jako główna metryka processed

Odrzucono. Polling jest używany wyłącznie do reconciliation; główny timestamp jest pobierany natychmiast po matching update z Yellowstone.

### Prawdziwy BUY pump.fun

Odrzucono. Wprowadzałby ryzyko rynkowe i dodatkowe zmienne (quote, slippage, ATA, program-account contention), niepotrzebne do mierzenia transportu bundle.

### Warm Direct Jito channel w wyniku podstawowym

Odrzucono. Zmieniłby to, co robi kompatybilny `trigger::JitoClient`, który tworzy świeży kanał dla submitu.

### Automatyczny retry lub automatyczna podwyżka tipu

Odrzucono. Złamałyby twardy limit kosztu, mieszały attribution i mogły ukryć błąd transportu.

## D6. Konsekwencje

Uzyskano realny dowód, że obecny plan NLN udostępnia śledzalne `sendBundle` z bundle ID i status API. W krótkiej próbce NLN szybciej ACK-uje i ma niższą medianę do gRPC `processed`; oba lane osiągnęły pełną finality.

Nie wynika z tego decyzja o zmianie aktywnego Ghost sendera. Nie potwierdzono semantyki NLN `bundleOnly`, revert protection, wyłącznego routingu Jito ani przewagi w warunkach prawdziwego pump.fun launch contention. Wynik jest zależny od lokacji hosta, leader schedule, n=3 i wspólnego writable payera.

## D7. Inwarianty

Zachowane:

- brak zmian `MaterializedFeatureSet`, Gatekeepera, BUY/REJECT, selectorów i polityki ryzyka;
- brak zmian aktywnego sendera, configu, retry/failover policy lub production confirmation/reconciliation;
- oddzielny proces benchmarku, bez uruchamiania Ghost runtime i bez wpięcia w Event Bus;
- `submit`, ACK, Yellowstone `processed`, landing, finality i unknown są rozdzielone;
- secret API key oraz prywatny keypair pozostały wyłącznie w environment/procesie; nie są zapisane w kodzie, ADR ani raporcie;
- koszty są bounded przed submitem i wszystkie transakcje mają odrębne signatures.

## D8. Bramka akceptacyjna i follow-up

Benchmark jest zaakceptowany jako dowód równoważnego porównania dwóch bundle transportów, ponieważ:

1. lane B użył rzeczywistego NLN `sendBundle`, nie fallbacku `sendTransaction`;
2. oba lane zwróciły bundle ID i dały się niezależnie zrekoncyliować;
3. główna metryka processed pochodzi z Yellowstone, nie z wolnego pollingu;
4. wszystkie 6 signatures są finalized z `err=null`;
5. całkowity koszt jest zgodny z deklarowanym capem i saldem;
6. test nie zmienił zachowania produkcyjnego Ghosta.

Następny krok wymaga odrębnej decyzji: porównać rzeczywistą aktywną ścieżkę Sender Ghosta z Direct Jito oraz NLN `sendBundle`, najlepiej na odseparowanych payerach. Nie należy traktować tego ADR jako zgody na automatyczne wpięcie NLN do Ghost runtime.

