NoLimitNodes Program Streams / gRPC — specyfikacja integracyjna dla Ghost/FSC

1. Charakter usługi

NoLimitNodes udostępnia dwa różne typy strumieni danych dla Solany:

raw Yellowstone gRPC

oraz

NLN Program Streams, czyli własny dekodowany event layer nad Solaną.

W kontekście FSC, Pump.fun telemetry i selective scoring bardziej interesujący jest drugi produkt: Program Streams. Nie jest to zwykły raw Yellowstone, tylko gotowy strumień zdekodowanych zdarzeń domenowych, takich jak PumpFun create, PumpFun trade, SOL/token transfers, DEX swaps itd.

Według materiałów NLN Program Streams są katalogiem wielu topiców / streamów dla programów Solany, dekodowanych w czasie rzeczywistym. W praktyce oznacza to, że klient nie musi samodzielnie parsować całego raw Yellowstone payloadu, rozpoznawać instrukcji, mapować kont ani dekodować danych programu. NLN wykonuje część tej pracy po stronie swojej infrastruktury i emituje gotowy semantyczny event.

Dla Ghost/FSC najważniejsze jest to, że NLN nie oddaje wyłącznie „surowej transakcji”, tylko gotowy event domenowy.

Przykład potwierdzonego eventu PumpFun trade po zdekodowaniu payload z base64:

{ "signature": "3Ermuo4Edjf75J7nKyM2kawYFGZAZfeK76sLPvyAnTvfBPALmV4xt8dzTGXFLXn75MAZPk9rxABMyJ3At5QXQqub", "tx_index": "70", "mint": "Bown28k3do55ce3JBYCr9M228DZsr3LxZi9BBybFpump", "sol_amount": "8418446", "token_amount": "330719451263", "user": "BWJxBMRtqqHhGtJmtUo7whFW2TBd9LPqxWDy6tJdSo8u", "timestamp": "1780009808", "virtual_sol_reserves": "26551096335", "virtual_token_reserves": "1043392990434519", "real_sol_reserves": "852634530", "real_token_reserves": "763492990434519", "fee_recipient": "GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS", "fee_basis_points": "95", "fee": "79976", "creator": "5y2ex68L41UiPjZXRrxsZJfPcTjkdQEKLexXvwXcq6Wo", "creator_fee_basis_points": "30", "creator_fee": "25256", "total_unclaimed_tokens": "0", "total_claimed_tokens": "0", "current_sol_volume": "0", "last_update_timestamp": "0", "ix_name": "sell", "slot": "422817405", "block_time": "1780009808" }

To oznacza, że parser warstwy domenowej Pump.fun działa po stronie NLN, a Ghost może konsumować gotowe pola:

signature, slot, mint, user, creator, ix_name, sol_amount, token_amount, virtual_sol_reserves, real_sol_reserves, block_time itd.

Dla FSC szczególnie istotne jest to, że potwierdzono również osobny transfer lane:

prod.rpc.solana.system.transfers

który emituje gotowe transfery między kontami, w tym native SOL.

2. Endpointy

Z dashboardu NLN wynika następujący podział endpointów:

RPC HTTPS: https://rpc.nln.clr3.org WebSocket: wss://ws.nln.clr3.org Raw gRPC / Yellowstone: grpc.nln.clr3.org:443 Program Streams: stream-1.nln.clr3.org:443

Dla Program Streams realnie przetestowany endpoint to:

stream-1.nln.clr3.org:443

Serwis gRPC:

nln.stream.v1.StreamService

Dostępne metody:

ListTopics Subscribe

Reflection działa. Potwierdzona odpowiedź:

grpc.reflection.v1.ServerReflection nln.stream.v1.StreamService

To jest istotne, ponieważ można introspektować serwis grpcurl bez ręcznego posiadania pliku .proto.

3. Autoryzacja

Autoryzacja odbywa się przez metadata/header gRPC:

x-api-key: <NLN_API_KEY>

Przykład grpcurl:

grpcurl \ -H "x-api-key: $NLN_API_KEY" \ stream-1.nln.clr3.org:443 list

Przykład Node.js:

const meta = new grpc.Metadata(); meta.add("x-api-key", process.env.NLN_API_KEY);

W Rust/Tonic trzeba analogicznie dodać metadata x-api-key do requestu.

Klucza API nie należy hardcodować w kodzie, logach, CLI history ani repo. W środowisku produkcyjnym powinien być trzymany wyłącznie w env/secrets managerze.

4. Operacje gRPC

4.1. ListTopics

Służy do pobrania dostępnych topiców.

Test:

grpcurl \ -H "x-api-key: $NLN_API_KEY" \ -d '{}' \ stream-1.nln.clr3.org:443 \ nln.stream.v1.StreamService/ListTopics

Realnie zwrócone topiki, istotne dla Ghost/FSC:

prod.rpc.solana.program.swaps prod.rpc.solana.pumpfun.create prod.rpc.solana.pumpfun.trade prod.rpc.solana.pumpfun.graduate prod.rpc.solana.pumpfun.transaction prod.rpc.solana.system.transfers prod.rpc.solana.system.blocks-confirmed prod.rpc.solana.program.swaps-partitionless prod.mat.solana.program.swaps.raydium_amm_v4 prod.mat.solana.program.swaps.raydium_clmm prod.mat.solana.program.swaps.raydium_cpmm prod.mat.solana.program.swaps.raydium_stable prod.mat.solana.program.swaps.orca_whirlpool prod.mat.solana.program.swaps.meteora_dlmm prod.mat.solana.program.swaps.meteora_pools prod.mat.solana.program.swaps.meteora_damm prod.mat.solana.program.swaps.pumpfun prod.mat.solana.program.swaps.pumpswap

Dla FSC najważniejsze są:

prod.rpc.solana.system.transfers prod.rpc.solana.pumpfun.trade prod.rpc.solana.pumpfun.create prod.rpc.solana.pumpfun.transaction

Minimalny zestaw dla aktywacji FSC:

system.transfers + pumpfun.trade + pumpfun.create

4.2. Subscribe

Subskrypcja jest server-streaming RPC. Klient wysyła jeden request, a serwer odsyła wiele wiadomości po tym samym kanale.

Przykład:

grpcurl \ -H "x-api-key: $NLN_API_KEY" \ -d '{"topic":"prod.rpc.solana.pumpfun.trade","format":"JSON"}' \ stream-1.nln.clr3.org:443 \ nln.stream.v1.StreamService/Subscribe

Format requestu używany w testach:

{ "topic": "prod.rpc.solana.pumpfun.trade", "format": "JSON" }

Response wrapper ma strukturę podobną do:

{ "topic": "prod.rpc.solana.system.transfers", "offset": "12075869149", "timestampMs": "1780009461141", "payload": "base64..." }

Ważne: payload jest base64. Po dekodowaniu zawiera właściwy event JSON.

Dekodowanie CLI:

grpcurl \ -H "x-api-key: $NLN_API_KEY" \ -d '{"topic":"prod.rpc.solana.pumpfun.trade","format":"JSON"}' \ stream-1.nln.clr3.org:443 \ nln.stream.v1.StreamService/Subscribe \ | jq -r '.payload' \ | head -n 1 \ | base64 -d \ | jq

5. Najważniejsze topiki dla Ghost/FSC

5.1. prod.rpc.solana.pumpfun.create

Cel: detekcja nowego tokena / launch event.

Zastosowanie:

launch anchor mint lifecycle start creator extraction start okna obserwacji powiązanie mint → creator inicjalizacja stanu pool/token

W pipeline:

PumpFun Create → pool/token lifecycle state → observation window start → Gatekeeper/scoring context

5.2. prod.rpc.solana.pumpfun.trade

Cel: obserwacja buy/sell w oknie early trading.

Zastosowanie:

early buyer set buy pressure sell pressure creator/funder correlation trade volume buyer window dla FSC bonding curve telemetry early behavior model

Potwierdzony payload zawiera:

signature tx_index mint sol_amount token_amount user timestamp virtual_sol_reserves virtual_token_reserves real_sol_reserves real_token_reserves fee_recipient fee_basis_points fee creator creator_fee_basis_points creator_fee total_unclaimed_tokens total_claimed_tokens current_sol_volume last_update_timestamp ix_name slot block_time

Dla Gatekeepera, scoringu i FSC istotne:

user = buyer/seller wallet mint = token ix_name = buy/sell sol_amount = nominalny przepływ SOL slot = porządek on-chain signature = dedupe/join key creator = creator tokena reserves = stan bonding curve

W kontekście FSC user z eventu pumpfun.trade jest kandydatem na early buyer wallet. Ten wallet jest potem lookupowany w rolling funding index zbudowanym z system.transfers.

5.3. prod.rpc.solana.system.transfers

Cel: rolling funding index dla FSC.

Zastosowanie:

source-of-funds first funder lookup funding concentration sybil/cabal detection wykrywanie dotowania buyer walletów wykrywanie fan-out funding pattern

Ten topic został realnie potwierdzony i zdekodowany.

Przykład rzeczywistego payloadu system.transfers po base64 decode:

{ "signature": "2EKxqUDAHEcUMsQtjJFwZSEd7jo2ysQkjhRMDKutAiVAWTaEmhHupuLGuBYaSsjeb8wpkdqVcASUnQ8KyPZQ1oH3", "tx_index": "408", "slot": "422819679", "from_wallet": "FVnv5qH7dsrBzEDwJ8dN2m9PFtKTBAQFtqWF3M9LpwMg", "to_wallet": "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe", "amount": "1000", "token_address": "solana", "instruction_index": 2 }

Potwierdzone pola:

signature tx_index slot from_wallet to_wallet amount token_address instruction_index

To potwierdza, że NLN Program Streams mogą bezpośrednio feedować FSC.

Przypadek:

Wallet A → 0.2 SOL → Wallet B Wallet A → 0.2 SOL → Wallet C Wallet A → 0.2 SOL → Wallet D Wallet A → 0.2 SOL → Wallet E ...

powinien być widoczny jako wiele eventów transferowych z tym samym from_wallet i różnymi to_wallet, o ile są to transfery emitowane przez prod.rpc.solana.system.transfers.

Dla FSC to jest niemal idealny format wejściowy:

from_wallet → to_wallet amount token_address slot signature instruction_index tx_index

W pipeline FSC:

prod.rpc.solana.system.transfers → decode TransferEntry → filter token_address == "solana" → filter amount >= min_funding_lamports → rolling TTL map: to_wallet -> Vec<FundingEvent> → buyer funding lookup → funding source concentration score

Minimalny model semantyczny:

buyer wallet = user z pumpfun.trade funding source = latest non-neutral SOL transfer into buyer within TTL

Czyli:

pumpfun.trade.user → lookup jako system.transfers.to_wallet → funding_source = system.transfers.from_wallet

Jeżeli 4 z 5 early buyerów ma tego samego from_wallet w ostatnich N minutach, FSC powinien wykryć wysoką koncentrację źródła finansowania.

5.4. prod.rpc.solana.pumpfun.transaction

Cel: metadata transakcji Pump.fun.

Potencjalne zastosowanie:

audit lane signature-level enrichment tx-level diagnostics fallback gdy trade/create stream nie daje pełnej informacji weryfikacja account keys / instrukcji / inner instructions

Do sprawdzenia: pełny payload i czy zawiera wszystkie account keys, instrukcje, inner instructions, status błędu i fee payer.

Ten stream nie jest krytyczny dla FSC V1, ale może być ważny jako enrichment/audit lane.

5.5. prod.rpc.solana.program.swaps / swaps-partitionless

Cel: szerszy DEX flow.

Zastosowanie:

post-graduation monitoring DEX-level telemetry migration follow-up Raydium/Meteora/PumpSwap activity broader market flow

Dla samego FSC nie jest krytyczne, ale może być użyteczne dla post-launch lifecycle i monitoringu tokenów po migracji z Pump.fun.

6. Potwierdzone możliwości feedowania FSC

6.1. FSC — definicja metryki

Funding Source Concentration mierzy, czy early buyers w oknie obserwacyjnym zostali sfinansowani przez to samo źródło, czyli przez ten sam wallet nadrzędny.

Motywacja:

Cabal / sybil cluster musi operacyjnie fundować buyer wallety. Jeżeli wiele pozornie niezależnych buyer walletów dostało SOL z tego samego źródła tuż przed launch/trade window, to jest to mocny sygnał cabal/sybil activity.

Przykład sygnału:

Buyer A ← 0.2 SOL ← Funder X Buyer B ← 0.2 SOL ← Funder X Buyer C ← 0.2 SOL ← Funder X Buyer D ← 0.2 SOL ← Funder X Buyer E ← unknown / other

Wynik:

top_funder = Funder X known funded buyers = 4 top_funder_count = 4 buyer_count = 5 FSC_count ≈ 0.8, zależnie od denominator policy

6.2. Potwierdzony input NLN dla FSC

NLN dostarcza dwa potrzebne typy danych:

A. Early buyer/trader events

Topic:

prod.rpc.solana.pumpfun.trade

Kluczowe pola:

user mint ix_name sol_amount signature slot tx_index creator block_time

Dla FSC:

user = buyer/seller wallet

Przy liczeniu FSC należy brać przede wszystkim eventy ix_name == "buy" w zadanym oknie obserwacyjnym po launchu.

B. Funding transfer events

Topic:

prod.rpc.solana.system.transfers

Kluczowe pola:

from_wallet to_wallet amount token_address signature slot instruction_index tx_index

Dla FSC:

to_wallet = potencjalny buyer wallet from_wallet = potencjalny funding source amount = wartość finansowania token_address = "solana" dla native SOL

To wystarcza do single-hop FSC.

6.3. Bezpośredni mechanizm FSC na NLN

Proponowany mechanizm:

1. Subskrybuj prod.rpc.solana.system.transfers. 2. Dekoduj payload base64 → JSON. 3. Filtrowanie: - token_address == "solana" - amount >= min_funding_lamports 4. Zapisuj event do rolling FundingIndex: to_wallet -> Vec<FundingEvent> 5. TTL: 300s. 6. Subskrybuj prod.rpc.solana.pumpfun.trade. 7. Dla każdego early buy: buyer = user funding_events = FundingIndex[buyer] funding_source = latest valid from_wallet 8. Agreguj funding_source dla buyerów w oknie obserwacyjnym. 9. Wylicz FSC_count / FSC_sol_weighted / coverage.

Minimalna struktura wejściowa:

struct FundingEvent { signature: String, slot: u64, tx_index: u32, instruction_index: u32, from_wallet: Pubkey, to_wallet: Pubkey, amount_lamports: u64, token_address: String, recv_ts_ns: u128, }

Indeks:

type FundingIndex = HashMap<Pubkey, SmallVec<[FundingEvent; 4]>>;

Klucz:

to_wallet

Czyli:

to_wallet -> lista ostatnich funding eventów

Dedupe key dla transferów:

(signature, instruction_index)

albo bezpieczniej:

(signature, instruction_index, from_wallet, to_wallet, amount)

instruction_index jest bardzo ważny, bo pozwala rozróżniać wiele transferów w jednej transakcji.

6.4. Amount filtering / dust protection

Potwierdzony transfer event miał:

amount = 1000 lamports token_address = "solana"

To jest pył, a nie realne finansowanie trading walleta.

Dlatego FSC musi mieć konfigurowalny próg:

min_funding_lamports

Rekomendowany start:

min_funding_lamports = 5_000_000 // 0.005 SOL

lub bardziej konserwatywnie:

min_funding_lamports = 10_000_000 // 0.01 SOL

Docelowy próg powinien być dobrany empirycznie na danych z replay/production telemetry.

Bez filtra amount FSC może zostać zanieczyszczony przez:

dust transfers spam techniczne mikropłatności rent / marginalne przepływy nieistotne transfery wewnętrzne

6.5. Neutral funders / CEX whitelist

Dla FSC konieczne jest rozróżnienie:

cabal funder vs neutral funder

Wiele organicznych portfeli może być zasilanych z CEX hot walletów. Jeżeli wielu buyerów dostało SOL z tego samego hot walleta Binance/Coinbase/Kraken, to nie powinno to automatycznie oznaczać cabal.

Dlatego potrzebny jest konfigurowalny:

neutral_funder_set

Przykładowa klasyfikacja:

if from_wallet in neutral_funder_set: funding_source_class = NEUTRAL_FUNDER else: funding_source_class = NON_NEUTRAL_FUNDER

Neutral funderów nie należy wrzucać do numeratorów FSC jako cabal source.

Rekomendacja:

CEX/known infra wallets: - wersjonowany config - możliwość hot reload - osobna metryka fsc_neutral_count - nie hardcodować w kodzie

6.6. FSC output fields

Rekomendowane outputy do Gatekeeper/evidence:

fsc_count fsc_sol_weighted fsc_known_coverage fsc_unknown_count fsc_neutral_count fsc_top_funder fsc_top_funder_count fsc_top_funder_buy_sol fsc_total_buyers fsc_known_funded_buyers fsc_non_neutral_known_buyers fsc_min_funding_lamports fsc_ttl_seconds

Dwie wersje FSC:

Count-based

FSC_count = max_buyers_same_funder / known_non_neutral_funded_buyers

SOL-weighted

FSC_sol_weighted = buy_sol_by_buyers_from_top_funder / total_buy_sol_by_known_non_neutral_funded_buyers

SOL-weighted jest ważniejszy decyzyjnie, bo buyer kupujący za 1 SOL powinien ważyć więcej niż buyer kupujący za 0.001 SOL.

6.7. Coverage policy

UNKNOWN nie powinno automatycznie zaniżać FSC bez raportowania coverage.

Przykład:

5 buyers total 3 known funded buyers 3 funded by same from_wallet 2 unknown

Nie należy interpretować tego ślepo jako FSC = 0.6 bez dodatkowych pól.

Lepszy output:

fsc_count = 1.0 over known_non_neutral_funded_buyers fsc_known_coverage = 0.6 fsc_unknown_count = 2

Gatekeeper powinien widzieć zarówno koncentrację, jak i coverage.

7. Wyniki testów wykonanych na NeuGhost

7.1. RPC latency

Endpoint:

https://rpc.nln.clr3.org

Metoda:

getSlot

Pomiar:

curl -s -o /dev/null \ -w "%{time_total}\n" \ -H "x-api-key: $NLN_API_KEY" \ -H "Content-Type: application/json" \ -d '{"jsonrpc":"2.0","id":1,"method":"getSlot"}' \ https://rpc.nln.clr3.org

Wynik po odcięciu cold-startu:

avg = 0.0733815 s p50 = 0.073799 s p90 = 0.081205 s p99 = 0.084543 s

Interpretacja:

RPC p50 ≈ 74 ms RPC p90 ≈ 81 ms RPC p99 ≈ 85 ms

To jest stabilne i ma niski jitter. Dla porównania użytkownik raportował Chainstack RPC około 200–800 ms w analogicznym teście z tej samej maszyny. To nie jest jeszcze dowód przewagi gRPC/event feedu, ale jest mocnym sygnałem jakości endpointu RPC.

7.2. Program Streams latency: system.transfers

Pomiar:

grpcurl \ -H "x-api-key: $NLN_API_KEY" \ -d '{"topic":"prod.rpc.solana.system.transfers","format":"JSON"}' \ stream-1.nln.clr3.org:443 \ nln.stream.v1.StreamService/Subscribe \ | jq -r --unbuffered ' .timestampMs as $server | (now*1000|floor) as $recv | "latency_ms=\($recv - ($server|tonumber)) offset=\(.offset)" '

Wynik obserwowany:

latency_ms ≈ 29–30 ms

Interpretacja:

To mierzy:

NLN SubscribeResponse.timestampMs → odbiór na NeuGhost

Nie mierzy pełnego:

Solana instruction → validator → NLN → NeuGhost

Nie wiemy jeszcze dokładnie, kiedy NLN nadaje timestampMs: przy odbiorze z validatora, po parsowaniu, czy tuż przed emisją do klienta. Natomiast jako pomiar transportu/emit-to-receive dla Program Streams wynik jest bardzo dobry.

7.3. Program Streams latency: pumpfun.trade

Analogiczny pomiar na:

prod.rpc.solana.pumpfun.trade

Wynik obserwowany:

typowo: 30–33 ms minimum: 27–29 ms sporadyczne skoki: 40–43 ms

Szacunkowo z obserwacji ekranu:

p50 ≈ 31 ms p90 ≈ 33 ms p99 ≈ 43 ms

Interpretacja: bardzo stabilny stream, niski jitter, brak widocznych przerw/dropów w krótkim teście.

7.4. First-seen względem Chainstack

Użytkownik raportuje, że:

Chainstack Program/stream latency: ~40–50 ms NLN: ~29–43 ms NLN widzi szybciej NLN brak dropów w obserwowanym teście

To jest silny praktyczny sygnał, ale do produkcyjnej kwalifikacji nadal należy utrzymać formalny dual-feed benchmark przez 24–72h.

W szczególności trzeba mierzyć:

nln_first_count chainstack_first_count tie_count delta_ms per signature missing_on_nln missing_on_chainstack offset_gap_count reconnect_count

7.5. Dropy / offset

Użytkownik raportuje brak dropów oraz ciągły offset w obserwacji. Krótkie testy nie pokazały problemu.

Dla produkcyjnej kwalifikacji trzeba logować:

topic offset prev_offset offset_gap timestampMs recv_ts_ms signature slot

Jeżeli offset jest monotoniczny i przyrostowy per topic, można wykrywać dziury:

offset_gap = current_offset - previous_offset - 1

Trzeba potwierdzić semantykę offsetu u NLN: czy offset jest per-topic, per-partition, globalny, czy może per-stream. Nie należy zakładać semantyki bez testu lub dokumentacji.

8. Proponowana architektura integracji z Ghost

8.1. Lane layout

Proponowany układ lane’ów:

Lane A: PumpFun Create topic: prod.rpc.solana.pumpfun.create purpose: lifecycle start / launch anchor Lane B: PumpFun Trade topic: prod.rpc.solana.pumpfun.trade purpose: early buyers, buy/sell pressure, bonding curve telemetry Lane C: System Transfers topic: prod.rpc.solana.system.transfers purpose: FSC rolling funding index Lane D: PumpFun Transaction topic: prod.rpc.solana.pumpfun.transaction purpose: audit/enrichment/fallback Lane E: DEX Swaps / post-graduation topic: prod.rpc.solana.program.swaps or protocol-specific topics purpose: migration, post-bonding lifecycle, broader market flow

Dla aktywacji FSC minimalny zestaw to:

system.transfers + pumpfun.trade + pumpfun.create

8.2. FSC integration

Aktualna logika FSC:

Funding Source Concentration: czy early buyers w oknie obserwacji byli fundowani przez to samo źródło

Integracja z NLN:

prod.rpc.solana.system.transfers → parse TransferEntry → filter token_address == "solana" → filter amount >= min_funding_lamports → maintain rolling HashMap<to_wallet, Vec<FundingEvent>> → TTL 300s → cap K funding events per wallet → eviction policy prod.rpc.solana.pumpfun.trade → parse TradeEntry → collect early buyers in observation window → for each buyer lookup funding source in rolling map → compute FSC

Struktura sugerowana:

struct FundingEvent { signature: String, slot: u64, tx_index: u32, instruction_index: u32, from_wallet: Pubkey, to_wallet: Pubkey, amount_lamports: u64, token_address: String, recv_ts_ns: u128, } type FundingIndex = HashMap<Pubkey, SmallVec<[FundingEvent; 4]>>;

Minimalna semantyka:

funding_source = latest non-neutral SOL transfer into buyer within TTL

Rekomendowane outputy FSC:

fsc_count fsc_sol_weighted fsc_known_coverage fsc_unknown_count fsc_neutral_count fsc_top_funder fsc_top_funder_count fsc_top_funder_buy_sol

Nie traktować FSC jako samodzielnego hard rejectu bez confidence/coverage. To powinien być mocny soft signal / Gatekeeper feature.

9. Dedupe, ordering i join keys

Dla każdego eventu należy logować:

provider = NLN topic offset timestamp_ms_nln recv_ts_ns_local signature slot tx_index, jeśli dostępne instruction_index, jeśli dostępne mint, jeśli dostępne event_type / ix_name

Główny dedupe key dla envelope/eventów:

(signature, tx_index, topic)

Dla PumpFun trade:

(signature, tx_index, ix_name, user, mint)

Dla transferów:

(signature, instruction_index)

Bezpieczniejsza wersja:

(signature, instruction_index, from_wallet, to_wallet, amount)

Ordering:

slot + tx_index + instruction_index

Dla FSC single-hop funding w oknie 300s zwykle wystarczy slot + recv order, ale do audytu należy zachować pełny event, w tym signature, tx_index i instruction_index.

10. Latency measurement: co mierzyć produkcyjnie

Nie wystarczy mierzyć timestampMs -> recv. To jest tylko latency od timestampu NLN do naszego procesu.

Należy mierzyć cztery klasy metryk.

10.1. Transport latency

transport_latency_ms = local_recv_ms - nln_timestampMs

Już zmierzone:

~29–43 ms dla Program Streams

10.2. First-seen provider delta

Jeśli równolegle działa Chainstack/raw Yellowstone:

delta_ms = recv_ts_nln - recv_ts_chainstack

Dla każdego signature.

Agregować:

nln_first_count chainstack_first_count tie_count delta_p50 delta_p90 delta_p99 missing_on_nln missing_on_chainstack

To jest najważniejszy test realnej przewagi feedu.

10.3. Coverage

Dla topiców krytycznych:

PumpFun create coverage PumpFun trade coverage System transfers coverage

Porównywać po signature z drugim providerem / raw Yellowstone / RPC audit.

10.4. Stability

Logować:

disconnect_count reconnect_count reconnect_duration_ms offset_gap_count payload_decode_error_count stale_event_count p99/p999 transport latency

Test kwalifikacyjny: minimum 24h, preferowane 72h.

11. Rust integration notes

Docelowo nie używać grpcurl ani CLI. Potrzebny jest natywny klient Rust oparty o tonic.

Warstwy:

nln_stream_client ├── connect() ├── list_topics() ├── subscribe(topic, format) ├── reconnect loop ├── heartbeat/stall detection ├── decode wrapper ├── base64 decode payload ├── serde_json deserialize typed event └── emit internal normalized event

Wewnętrzny output do pipeline’u Ghost:

enum NlnEvent { PumpFunCreate(PumpFunCreateEvent), PumpFunTrade(PumpFunTradeEvent), Transfer(TransferEvent), PumpFunTransaction(PumpFunTransactionEvent), Swap(SwapEvent), Unknown { topic: String, raw: serde_json::Value }, }

Wrapper:

struct NlnSubscribeEnvelope { topic: String, offset: String, timestamp_ms: i64, payload: String, // base64 encoded JSON }

Każdy event po normalizacji powinien dostać metadata:

struct IngestMeta { provider: ProviderId, // NLN topic: String, offset: Option<u64>, provider_ts_ms: Option<i64>, recv_ts_ns: u128, decode_ts_ns: u128, slot: Option<u64>, signature: Option<String>, }

Transfer event po normalizacji:

struct NlnTransferEvent { signature: Signature, tx_index: u32, instruction_index: u32, slot: u64, from_wallet: Pubkey, to_wallet: Pubkey, amount_lamports: u64, token_address: String, }

PumpFun trade event po normalizacji:

struct NlnPumpFunTradeEvent { signature: Signature, tx_index: u32, slot: u64, mint: Pubkey, user: Pubkey, creator: Pubkey, ix_name: PumpFunTradeSide, // buy/sell sol_amount: u64, token_amount: u64, block_time: Option<i64>, virtual_sol_reserves: Option<u64>, virtual_token_reserves: Option<u64>, real_sol_reserves: Option<u64>, real_token_reserves: Option<u64>, }

Uwaga: większość wartości liczbowych w JSON przychodzi jako string. Parser Rust powinien jawnie parsować string → u64/u128/i64, a nie zakładać natywnych typów JSON number.

12. Reconnect policy

Wymagane w produkcji:

initial backoff: 100–250 ms max backoff: 5s jitter: yes max silent interval: per-topic configurable

Dla aktywnych streamów typu system.transfers stall detection może być agresywne, np.:

if no event for > 2s during normal network activity → suspect stall

Dla rzadkich topiców typu pumpfun.create nie można używać prostego „no events” jako dowodu stalla, bo eventy naturalnie mogą być rzadsze.

Po reconnect:

log reconnect event increment reconnect_count record gap risk optionally audit recent signatures via secondary feed

Nie wiemy jeszcze, czy NLN Subscribe wspiera resume od offsetu. Jeżeli nie, delivery jest praktycznie live-only i trzeba traktować reconnect jako możliwe ryzyko luki.

13. Możliwości strategiczne dla Ghost

Największe korzyści NLN dla tego projektu:

system.transfers jako gotowy transfer lane pod FSC.
Potwierdzono, że payload zawiera from_wallet, to_wallet, amount, token_address, slot, signature, instruction_index.

Bezpośrednie wykrywanie funding fan-out.
Schemat Wallet A → wiele buyer wallets może być wykrywany bez własnego dekodowania raw Yellowstone, przez agregację from_wallet w transfer lane.

pumpfun.trade jako gotowy buyer/seller lane.
Dostajemy user, mint, ix_name, sol_amount, creator, reserves, slot, signature.

Naturalny join dla FSC.
pumpfun.trade.user może być lookupowany jako system.transfers.to_wallet.

Niski measured transport latency.
W testach: około 30 ms Program Streams do NeuGhost.

Oddzielne topiki zamiast jednego szerokiego streamu.
Można rozdzielić workloady: founding, trading, funding, audit.

Niższy koszt architektoniczny dla prototypowania nowych metryk.
Mniej pracy nad parserami, więcej nad scoringiem, coverage i signal quality.

Możliwość utrzymania Chainstack jako audit/fallback.
Nie trzeba od razu migrować wszystkiego. Można zrobić równoległy dual-feed.

14. Ryzyka i rzeczy do potwierdzenia

Najważniejsze rzeczy, których nie wolno założyć bez testu:

Pełna semantyka timestampMs.
Nie wiemy, czy to czas odbioru z validatora, czas po decode, czy czas emisji z serwera NLN.

Semantyka offset.
Trzeba potwierdzić, czy offset jest per-topic, per-partition, globalny i czy można po nim wznawiać stream.

Pełna coverage system.transfers.
Potwierdzono payload i obecność SOL transferów, ale trzeba sprawdzić coverage względem raw Yellowstone/Chainstack.

Multi-transfer transactions.
Event ma instruction_index, co jest dobrym sygnałem, ale trzeba sprawdzić reprezentację transakcji z wieloma transferami.

Inner instructions.
Dla FSC trzeba potwierdzić, czy transfery z inner instructions są emitowane.

Token transfers vs SOL transfers.
Topic opisany jest jako „Solana token and SOL transfers”. Dla FSC najważniejsze są native SOL funding transfers, czyli token_address == "solana".

Dust/spam transfers.
Potwierdzony przykład miał amount = 1000 lamports, więc konieczny jest min_funding_lamports.

Delivery guarantees.
Brak potwierdzonej semantyki replay/resume. Do czasu potwierdzenia traktować jako live stream z lokalną detekcją luk.

Vendor lock-in parserowy.
Program Streams są wygodne, ale parser jest po stronie NLN. Dla krytycznych decyzji warto utrzymać raw/audit lane.

15. Minimalny plan wdrożenia

Etap 1: klient NLN Program Streams

Implement tonic client ListTopics sanity check Subscribe pumpfun.trade Subscribe system.transfers Decode base64 payload Deserialize JSON Normalize to internal events

Etap 2: transfer lane / funding index

system.transfers → parse TransferEntry → filter token_address == "solana" → filter amount >= min_funding_lamports → insert into FundingIndex[to_wallet] → TTL 300s → cap K events per wallet → eviction policy

Etap 3: FSC activation

pumpfun.create → lifecycle start pumpfun.trade → early buyer collector system.transfers → rolling funding map on pool analysis: buyers = early buy users for each buyer: funding_source = latest valid FundingIndex[buyer] compute: fsc_count fsc_sol_weighted fsc_known_coverage fsc_unknown_count fsc_neutral_count fsc_top_funder fsc_top_funder_count fsc_top_funder_buy_sol emit fsc_* fields into Gatekeeper evidence

Etap 4: benchmark dual-feed

NLN vs Chainstack same signatures same slots recv_ts_ns comparison coverage comparison offset gap monitoring 24h run minimum 72h preferred

Etap 5: production policy

NLN primary for FSC/funding lane if: - zero or negligible missing coverage - p99 stable - no unexplained offset gaps - first-seen advantage persists - reconnect behavior acceptable - system.transfers coverage acceptable - no unacceptable parser inconsistency Chainstack remains: - fallback - audit - raw decode verification

16. Rekomendowana decyzja techniczna

Na podstawie obecnych testów NLN wygląda bardzo mocno jako primary candidate dla FSC/funding lane oraz Pump.fun telemetry lane.

Najważniejsza zmiana względem wcześniejszej oceny: potwierdzono, że prod.rpc.solana.system.transfers emituje transfery z polami wymaganymi dla FSC:

from_wallet to_wallet amount token_address slot signature instruction_index tx_index

To oznacza, że NLN może bezpośrednio zasilać FSC bez konieczności własnego dekodowania raw Yellowstone dla podstawowego single-hop funding source lookup.


Po checku:

p50 = 166000 lamports to tylko 0.000166 SOL, czyli masa eventów to pył / techniczne drobne transfery / noise.
p90 = 1.844631125 SOL jest już duże.
p99 = 11.214403503 SOL, max = 13.380597185 SOL pokazuje, że stream łapie realne duże przepływy.
Dla FSC to oznacza, że min_funding_lamports jest obowiązkowy. Bez tego funding index będzie zanieczyszczony pyłem.
Rekomendowałbym startowo trzy progi jako warianty eksperymentalne:

FSC_A: min_funding_lamports = 5_000_000      // 0.005 SOL
FSC_B: min_funding_lamports = 10_000_000     // 0.01 SOL
FSC_C: min_funding_lamports = 50_000_000     // 0.05 SOL

Po drugie, FSC implementation musi być bounded. Przy ~1270 events/s:
bo:

events = 10000
ms = 7868
eps = 1270.97

Czyli:

Observed transfer topic composition:
- system.transfers includes native SOL, wrapped SOL and SPL token transfers.
- Native SOL is represented as token_address == "solana".
- Wrapped SOL appears as So11111111111111111111111111111111111111112.
- FSC V1 should use only token_address == "solana" as primary funding source input.
- WSOL/SPL transfers should be treated as separate optional enrichment, not mixed into FSC primary score.

Observed amount distribution for native SOL sample:
- n = 346
- min = 0
- p50 = 166000 lamports
- p90 = 1844631125 lamports
- p99 = 11214403503 lamports
- max = 13380597185 lamports

Conclusion:
- dust/noise is significant
- min_funding_lamports is mandatory
- recommended initial threshold sweep: 0.005, 0.01, 0.05 SOL

Observed throughput:
- system.transfers sample: 10,000 events in 7.868s
- approx. 1,271 events/s

Implication:
- raw transfer lane must not be stored unbounded
- apply early filters before FundingIndex insert
- maintain TTL eviction
- cap events per receiver wallet

DODATKOWE OBSERWACJE:

Confirmed:
SubscribeResponse exposes partition and offset.

Observed:
partition = 0 for system.transfers.
offset is monotonic but non-contiguous, with large jumps.

Conclusion:
offset is not a reliable per-topic sequential event counter.
Do not use offset gaps as direct evidence of dropped stream messages unless NLN confirms offset semantics.
Use offset for logging, ordering diagnostics, and duplicate analysis only.

Confirmed:
- SubscribeRequest supports only topic + format.
- No offset/cursor resume in request.
- SubscribeResponse includes topic, partition, offset, timestamp_ms, payload.
- OutputFormat supports JSON and PROTO.
- Payload is bytes.
- TopicInfo exposes proto_message_type.

PROTO mode exists, but payload message schemas must be confirmed.
If schemas are exposed via reflection, Rust should prefer PROTO mode for lower decode overhead.
If schemas are not exposed, production V1 can use JSON mode and keep PROTO as optimization path.

Confirmed:
- StreamService exposes ListTopics and Subscribe.
- Subscribe is live-only by schema.
- Request supports only topic + format.
- Response includes topic, partition, offset, timestamp_ms, payload.
- No client-driven resume/replay via offset.
- Payload is bytes; grpcurl JSON renders it as base64.

SubscribeResponse zawiera partition + offset, ale SubscribeRequest nie wspiera offset/cursor resume.
Offset służy do telemetry/detekcji luk/dedupe/diagnostyki, nie do client-side replay.

Confirmed:
- OutputFormat supports JSON and PROTO.
- PROTO payload is emitted as raw protobuf bytes in SubscribeResponse.payload.
- grpcurl renders bytes payload as base64.
- Payload message schemas such as TransferEntry are not exposed via reflection.
- Therefore Rust V1 should use JSON mode unless NLN provides .proto schemas for payload events.
- Offset is exposed and can be locally contiguous, but SubscribeRequest has no offset/cursor field, so client-side resume/replay is not supported by the public gRPC contract.

Confirmed limitation:
NLN RPC endpoint tested successfully for low-latency getSlot, but does not currently provide usable transaction/block audit for sampled Program Stream events. getSignatureStatuses returned null for native SOL and PumpFun trade samples, getTransaction returned null/rate-limit, and getBlock for sampled slot returned null.

Implication:
Do not depend on NLN RPC for transaction-history validation, WSOL context classification, or getBlock-based replay/audit. Treat NLN RPC as fast operational RPC, not as the primary historical audit/indexing backend.

Production policy:
Use NLN Program Streams as live feed. Use Chainstack/raw Yellowstone/another archive-capable RPC as audit/fallback for transaction reconstruction, coverage verification, and WSOL/inner-instruction classification.

FSC V1 remains valid on NLN Program Streams:
- pumpfun.trade gives buyer/user
- system.transfers gives native SOL from_wallet -> to_wallet
- join buyer=user to transfer.to_wallet

Do not enrich FSC V1 by calling NLN getTransaction.
Do not include WSOL in primary FSC until classified through another audit source.
Do not use NLN RPC to prove coverage.