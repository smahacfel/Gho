# ADR-8D: Kontrolowany benchmark submission i landing — Jito Frankfurt vs NLN RPC, 2026-07-22

## Status

Accepted — jednorazowa, jawnie zatwierdzona próba live została zakończona i zrekoncyliowana.

## D1. Problem

Poprzedni benchmark rozdzielał gRPC/RPC transport i odbiór Pump.fun, ale nie mógł uczciwie odpowiedzieć na pytanie o rzeczywistą ścieżkę Jito lub submit przez NLN. Sam HTTP ACK nie oznacza ani forwardingu, ani landingu. Opis NLN deklaruje leader-aware forwarding i Jito-aware routing, lecz publiczny standardowy interfejs sendTransaction nie daje w odpowiedzi bundle_id ani dowodu wybranej ścieżki.

Wymagane były jednocześnie:

- kontrolowany, mały koszt i osobna zgoda na live podpis;
- rozdzielenie attribution lane, bez wspólnej sygnatury i AlreadyProcessed;
- jasne proof layers: HTTP ACK, Jito bundle status, Solana signature status, landed slot i koszt;
- brak ingerencji w aktywny Ghost, Gatekeeper, config i sender runtime.

## D2. Decyzja

Dodano odizolowany Rust example:

off-chain/components/seer/examples/nln_jito_submission_benchmark.rs

Narzędzie wymaga jawnego flag --execute oraz obu sekretów przekazywanych wyłącznie przez environment:

- NLN_BENCHMARK_API_KEY;
- JITO_PROBE_KEYPAIR_PATH.

Zbudowano po jednej świeżej transakcji dla lane Jito i NLN. Każda ma:

- identyfikujące Memo, inne dla każdego lane;
- ComputeBudget: 50,000 CU i 25,000 µlamports/CU;
- inline transfer 2,000,000 lamportów do aktualnego Jito tip account;
- zero token trade, zero wywołania Ghost runtime, zero retry i zero automatycznej eskalacji.

Jito używa Frankfurt endpoint i sendBundle z jedną podpisaną transakcją. NLN używa standardowego sendTransaction z base64, skipPreflight=true, maxRetries=0 i minContextSlot pobranym z aktualnego blockhash. Status jest weryfikowany przez Jito getInflightBundleStatuses, Jito getBundleStatuses oraz Solana getSignatureStatuses. Wersja po wyniku obsługuje Inflight Invalid jako stan wymagający getBundleStatuses, a nie jako końcowy failure.

Wynik zbiorczy zapisano w:

PLANS/AUDYT/RAPORT_BENCHMARK_NLN_GRPC_RPC_LATENCY_20260722.md

## D3. Kontekst

Zgoda użytkownika obejmowała pojedynczą wysyłkę bundle do Jito i przez RPC sender NLN, z tym samym tip/fee. Bieżący hard cap configu Ghosta dla tip wynosi 0.002 SOL. Harness zachowuje ten default. Tip 0.003–0.004 SOL nie może wystąpić bez jawnego argumentu i --allow-escalated-tip, nawet jeżeli operator wcześniej dopuścił taką eskalację po reject.

Jito dokumentuje, że ACK sendBundle oznacza przyjęcie bundle przez Block Engine, nie gwarancję landingu; getBundleStatuses daje slot i confirmation. Solana dokumentuje, że sendTransaction jest akceptacją RPC, a getSignatureStatuses służy do rozstrzygnięcia on-chain outcome. Te rozróżnienia są centralne dla raportu.

## D4. Dowody

Próba z 2026-07-22T15:33:40.775Z:

| Lane | ACK | Landed/finalized slot | Fee | Tip | Evidence |
| --- | ---: | ---: | ---: | ---: | --- |
| Jito Frankfurt sendBundle | 5.54 ms | 434535725 | 6,250 lamportów | 2,000,000 lamportów | bundle_id + Jito getBundleStatuses + Solana signature status |
| NLN RPC sendTransaction | 11.42 ms | 434535734 | 6,250 lamportów | 2,000,000 lamportów | returned signature + Solana signature status |

Oba signatures są finalized, z err=null. Saldo testowego keypaira zmieniło się z 47,172,000 do 43,159,500 lamportów, dokładnie o dwa razy tip plus fee.

Krótka, identyczna seria rozszerzająca podniosła reconciliation do 5 / 5 finalized signature dla direct Jito code path i 5 / 5 dla NLN. Łączny końcowy koszt pięciu par to 20,062,500 lamportów, a saldo testowego keypaira po serii to 27,109,500 lamportów. Twardy provider-level bundle_id oraz measured ACK są zachowane dla pierwszej pary; rozszerzenie jest dowodem finality/kosztu, nie podstawą do wyprowadzania p50/p95 ACK.

Ważna obserwacja kontraktowa: getInflightBundleStatuses Jito zwrócił Invalid dla bundle, który getBundleStatuses oraz Solana później potwierdziły jako finalized w slocie 434535725. Z tego powodu inflight status nie jest samodzielnym dowodem failure.

Walidacja implementation:

- cargo fmt --all -- --check;
- cargo check -p seer --example nln_jito_submission_benchmark;
- cargo run -p seer --example nln_jito_submission_benchmark -- --help;
- git diff --check.

Każdy check przeszedł. W workspace występują uprzednie warnings ghost-core, trigger i seer, ale nowy example nie ma własnego warningu.

## D5. Odrzucone alternatywy

### Wspólna sygnatura wysłana równolegle do Jito i NLN

Odrzucono. Pierwszy landing sprawia, że obserwacja drugiej trasy degeneruje się do AlreadyProcessed; nie da się wtedy wiarygodnie przypisać resultu do transportu.

### Uruchomienie prawdziwego BUY pump.fun

Odrzucono. Wnosiłoby slippage, pool accounts, ATA, contention i ryzyko rynku. Celem była ścieżka submission/landing, nie test strategii ani otwarcie pozycji.

### Uznanie ACK za sukces

Odrzucono. ACK Jito oznacza odbiór, a ACK Solana RPC oznacza przyjęcie przez RPC. Oba wymagają niezależnej reconciliation.

### Uznanie Jito Inflight Invalid za failure

Odrzucono po bezpośrednim kontrprzykładzie tej próby: ten sam bundle był finalized według getBundleStatuses i Solana.

### Automatyczne retry albo automatyczna eskalacja tipu

Odrzucono. Mogłyby produkować niejawne kosztowne próby, mieszać attribution i eskalować opłatę poza czytelną intencją operatora.

## D6. Konsekwencje

Operator ma powtarzalny benchmark, który mierzy local submit-to-ACK i dowodzi landed slotu. Zostało realnie potwierdzone:

- działanie direct Jito Frankfurt bundle path;
- działanie standardowego NLN sendTransaction;
- identyczny koszt obu kontrolowanych lane;
- konieczność wieloźródłowego confirmation/reconciliation dla Jito.

Nie zostało potwierdzone:

- dokładny czas ACK do execution;
- p50/p95/p99, SLO lub zwycięzca latency;
- Jito mirroring przez NLN lub bundleOnly semantics;
- bezpieczeństwo równoległego production BUY przez dwa lane.

## D7. Inwarianty

Zachowane:

- brak zmian MaterializedFeatureSet, Gatekeepera, BUY/REJECT, selector score i polityki ryzyka;
- brak zmiany aktywnego sendera, configu, runtime, log schema lub shadow/live boundary;
- testowy keypair i API key tylko w pamięci procesu; bez sekretu w report, code lub tracked config;
- jedna transakcja na lane, brak resubmission, ograniczona eskalacja i świeży blockhash per attempt;
- submit, landing oraz confirmation rozdzielone w raporcie;
- unknown i conflicting provider status nie są traktowane jako success bez dowodu chainowego.

## D8. Bramka akceptacyjna i follow-up

Benchmark jest zaakceptowany jako dowód pojedynczego, kontrolowanego execution probe, ponieważ:

1. obie transakcje mają niezależne signatures i pełną attribution lane;
2. koszt jest ujęty i zgodny ze stanem portfela;
3. Jito ma bundle_id oraz finalized evidence;
4. NLN ma returned signature oraz finalized evidence;
5. raport wyraźnie nie miesza ACK, landed slot i confirmed observation;
6. naprawiono discovered status-handling issue bez nowego live submitu.

Kolejny etap, tylko po osobnej decyzji projektowej, to seria ograniczona w czasie i liczbie prób: ustalona liczba slotów/leader windows, trzy lane (direct Jito, NLN, current sender), uniknięcie common payer account contention i definicja pre/post metrics. Nie należy automatycznie podłączać harnessu do Ghost execution.
