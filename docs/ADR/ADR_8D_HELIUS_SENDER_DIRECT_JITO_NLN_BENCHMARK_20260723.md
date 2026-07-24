# ADR-8D: Izolowany benchmark Helius Sender vs Direct Jito gRPC vs NLN sendBundle — 2026-07-23

## Status

Accepted — jeden kontrolowany run live został wykonany, zrekoncyliowany i udokumentowany. Nie zmienia produkcyjnego sendera Ghosta, configu, BUY/SELL ani polityki execution.

## D1. Problem

Ghost potrzebował miarodajnego, małego porównania rzeczywistej aktywnej ścieżki Helius Sender z Direct Jito gRPC i usługą NLN sendBundle. Wcześniejsze porównanie Direct/NLN nie obejmowało faktycznego LiveTxSender.

Ryzyko było dwojakie:

- manualnie ułożony request Helius mógłby nie mierzyć produkcyjnego kontraktu;
- provider ACK mógłby zostać błędnie uznany za landing lub finality.

## D2. Decyzja

Dodano wyłącznie ręcznie uruchamiany example:

ghost-launcher/examples/helius_sender_direct_jito_nln_benchmark.rs

Example:

- importuje prawdziwy LiveTxSender i wywołuje jego send_transaction oraz confirm_submission bez modyfikacji ich semantyki;
- używa publicznego trigger::JitoClient::submit_bundle bez ręcznej imitacji gRPC;
- używa faktycznego NLN JSON-RPC sendBundle z persistentnym HTTP clientem;
- buduje testową transakcję ComputeBudget + Memo + inline tip;
- wymaga prawidłowych symulacji wszystkich trzech payloadów przed każdym płatnym tripletem;
- ma hard cap i bramkę Stage 1 przed Stage 2;
- rozdziela ACK, processed receipt, on-chain landing/finality i production confirmation Helius;
- zapisuje wynik do raportu audytowego, bez wiring do runtime.

Raport wynikowy:

PLANS/AUDYT/RAPORT_BENCHMARK_HELIUS_SENDER_VS_DIRECT_JITO_VS_NLN_20260723.md

## D3. Kontekst

Aktualny origin/main tworzy LiveTxSender raz i aktywne BUY oraz SELL używają go do send_transaction oraz confirmation. Helius lane musiał zatem wywołać dokładnie ten obiekt, przy jednym persistentnym HTTP clientcie, bez benchmarkowego prewarm /fast.

Direct Jito jest zachowanym transportem Triggera. Jego publiczna metoda buduje świeży TLS/gRPC channel na submit i zachowuje istniejący failover przed ACK. Chociaż wewnętrznie dostaje Jito UUID, publiczna metoda zwraca lokalną signature; benchmark nie zmieniał API tylko po to, aby uzyskać UUID.

Operator jawnie zastąpił wymóg trzech niezależnych payerów jednym testowym walletem. To zmniejsza izolację porównania i musi być widoczne w interpretacji.

## D4. Dowody

Stage 0 przeszedł bez on-chain submita:

- Helius /ping: HTTP 200;
- Direct Jito i NLN getTipAccounts: po 8 kont;
- trzy lane otrzymały różne konta tipowe;
- trzy podpisane transakcje przeszły simulateTransaction, każda zużyła 25 470 CU;
- persistentny processed Yellowstone observer był uzbrojony przed submitami;
- saldo payera wystarczało na dokładnie wyliczony hard cap 9 056 250 lamportów.

Run live zawierał trzy triplety, dziewięć transakcji. Wszystkie 9/9 zostały finalized z on-chain err=null i kosztowały po 1 006 250 lamportów. Saldo spadło dokładnie z 15 079 200 do 6 022 950 lamportów, czyli o hard cap 9 056 250.

Helius miał w próbie mean ACK 22,504 ms oraz mean submit → processed receipt 139,086 ms. NLN miał najszybszy mean ACK 12,119 ms. Direct Jito miał mean ACK 40,561 ms. Wszystkie lane miały 3/3 finality. Pełne dane raw, sloty, bundle IDs i limitation znajdują się w raporcie.

Tylko pierwszy triplet miał start gap <=10 ms; dwa pozostałe są jawnie opisane jako diagnostyczne. Nie podano p95/p99 dla n=3.

Walidacja example:

- cargo fmt --all -- --check — PASS;
- cargo check -p ghost-launcher --example helius_sender_direct_jito_nln_benchmark — PASS;
- cargo test -p ghost-launcher --example helius_sender_direct_jito_nln_benchmark — PASS;
- cargo build -p ghost-launcher --example helius_sender_direct_jito_nln_benchmark — PASS;
- git diff --check — PASS.

## D5. Odrzucone alternatywy

### Ręczny request HTTP przypominający Helius Sender

Odrzucono. Nie mierzyłby prawdziwej produkcyjnej metody ani jej kontraktu response signature i confirmation.

### Modyfikacja LiveTxSender dla osobnych timestampów connect/subscription

Odrzucono. Użytkownik zakazał zmiany produkcyjnego sendera. W wyniku fresh connection duration jest null, a zmierzony jest cały prawdziwy confirm_submission.

### Użycie submit_bundle_with_redundancy_receipt dla Direct Jito UUID

Odrzucono. Ten interfejs świadomie wysyła wielokrotnie, co naruszałoby zasadę braku resubmitu po stronie benchmarku. Użyto publicznego submit_bundle as-is.

### Wspólny tip account albo wspólna signature

Odrzucono. Lane mają niezależne signatures i trzy różne tip accounts. Wspólny payer jest jedynym zaakceptowanym wyjątkiem.

### Polling RPC jako primary processed metric

Odrzucono. Primary metric to pierwsze lokalne matching processed z persistentnego Yellowstone. RPC i bundle status są tylko reconciliation.

### Pump.fun BUY albo uruchomienie Ghost runtime

Odrzucono. Dodawałyby ryzyko rynkowe, account contention programu i niepotrzebne zmienne, niepotrzebne do testu transportu.

## D6. Konsekwencje

Aktywna ścieżka Helius Sender pozostaje primary. Ten run dostarcza dowodu, że NLN sendBundle działa, zwraca bundle ID i daje bardziej kompletne provider attribution niż exact public Direct Jito wrapper. NLN można rozważać wyłącznie jako future redundant candidate, nie jako automatyczny replacement.

Nie wynika z tego, że Helius, NLN albo Direct Jito ma stałą przewagę dla Pump.fun. Wynik ma n=3, jeden shared payer i jeden primary observer.

## D7. Zachowane inwarianty

- brak zmian MaterializedFeatureSet, Gatekeepera, BUY/REJECT, selectorów i polityki ryzyka;
- brak zmian LiveTxSender, config.toml, live BUY/SELL wiring, retry/failover policy lub production confirmation;
- brak uruchomienia Ghost runtime;
- submit, ACK, processed, landing, finality i unknown są rozdzielone;
- timeout/transport error confirm_submission nie byłby uznany za brak landingu bez on-chain reconciliation;
- żaden secret, private key ani raw transaction bytes nie trafił do source, ADR ani raportu;
- hard cap był sprawdzony przed płatnym runem i końcowe saldo go potwierdziło;
- podczas retry symulacji rate limitu nie wysłano żadnego on-chain submita.

## D8. Bramka akceptacyjna i follow-up

Benchmark jest zaakceptowany jako dowód punktowy, ponieważ:

1. Helius lane użył rzeczywistego kontraktu active Ghost sendera;
2. Direct lane użył obecnego publicznego Trigger Jito transportu;
3. NLN lane użył rzeczywistego sendBundle, nie fallbacku sendTransaction;
4. wszystkie transakcje przeszły symulację, finalized i zgadzają się z saldem;
5. Stage 2 rozpoczął się wyłącznie po pozytywnej bramce Stage 1;
6. benchmark nie wpłynął na zachowanie runtime.

Ewentualny następny krok wymaga osobnej decyzji: większy burn-in z trzema odseparowanymi payerami, zachowaniem <=10 ms dla wszystkich paired triplettów oraz dopiero potem decyzja o redundant-lane integration. Niniejszy ADR nie daje zgody na zmianę primary lane ani runtime wiring.

