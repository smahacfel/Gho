DO ZREALIZOWANIA PO WYELIMINOWANIU WAZNIEJSZYCH PROBLEMOW:

**Najważniejsze**
- `base_mint="unknown"` w obecnym pipeline powstaje przede wszystkim w runtime fallback, nie w parserze. Źródło to [oracle_runtime.rs](/root/Gh/ghost-launcher/src/oracle_runtime.rs#L2535), gdzie `ObservationIdentity` dostaje `"unknown"`, gdy task startuje bez metadata.
- To pasuje do logów: masz seryjne `POOL_IDENTITY_FALLBACK ... reason=UNKNOWN_IDENTITY`, a chwilę później dla części tych samych pooli pojawia się `POOL_TASK_LATE_METADATA`. To jest problem kolejności / task auto-spawn, nie parsera.
- `dev_pubkey="unknown"` to częściowo celowa sanitacja, nie utrata danych pod loadem. Parser odrzuca creatorów, którzy nie wyglądają na sensowny owner w [binary_parser.rs](/root/Gh/off-chain/components/seer/src/binary_parser.rs#L317), a wrapper Seera zamienia to na `"unknown"` w [seer.rs](/root/Gh/ghost-launcher/src/components/seer.rs#L31).

**Ocena Twoich punktów**
- `Side-effecty w parse_trades`
  Aktualny kod rzeczywiście mutuje rejestr w parserze, ale nie widzę tu głównego źródła `unknown`.
  Uwaga: ta mutacja jest dziś też funkcjonalnie użyteczna, bo pozwala rozwiązać mapping w obrębie tego samego parse-pass dla CPI create/trade w [binary_parser.rs](/root/Gh/off-chain/components/seer/src/binary_parser.rs#L3048).
  Wniosek: architektonicznie do poprawy, ale nie to generuje obecne bloki `unknown`.
- `is_candidate_owner` i `bs58::encode` w hot path`
  To są realne koszty CPU w [binary_parser.rs](/root/Gh/off-chain/components/seer/src/binary_parser.rs#L3802) i [binary_parser.rs](/root/Gh/off-chain/components/seer/src/binary_parser.rs#L2705).
  Mogą pogarszać throughput parsera, ale nie mam dowodu, że to dziś powoduje te konkretne `unknown`.
  W logach nie znalazłem `LAG ORACLE`, więc nie mam potwierdzenia na parser-induced overload.
- `market_cap_sol: None` dla PumpSwap CPI`
  To jest brak kompletności danych downstream, ale nie ma związku z `mint/dev unknown`.
- `resolve_trade_mint_from_token_accounts` O(n²)`
  Prawdziwe, ale to raczej koszt cold-path niż źródło obecnego zjawiska.
- `dedup_trade_candidates` O(n²)`
  Niski priorytet. Bardziej elegancja niż realny winowajca.
- `ResolveQueue` bez TTL`
  To jest sensowny correctness hardening, ale symptom byłby raczej “stare złe mapowanie”, nie bursty bloków `unknown`.
- `bs58`/string allocacje`
  Warte optymalizacji, ale wtórne wobec już znalezionego problemu runtime.

**Co jest bardziej prawdopodobnym winowajcą**
- `tx-first` auto-spawn bez metadata w [oracle_runtime.rs](/root/Gh/ghost-launcher/src/oracle_runtime.rs#L5028)
- myląca semantyka okna, która wcześniej oznaczała `NewPoolDetected` nawet dla tx-first path w [oracle_runtime.rs](/root/Gh/ghost-launcher/src/oracle_runtime.rs#L4061)
- sanitacja creatorów do `"unknown"` po stronie Seera/parsingu, co jest osobnym zjawiskiem niż `base_mint unknown`

**Priorytet praktyczny**
- Jeśli pytasz “co teraz najbardziej poprawi jakość danych?”:
  1. runtime/order fixes, które już robimy,
  2. telemetry rozróżniająca `unknown because tx-first` vs `unknown because sanitized creator`,
  3. dopiero potem optymalizacje parsera hot-path.
- Jeśli pytasz “co z listy parsera warto zrobić mimo wszystko?”:
  1. `is_candidate_owner` bez `to_string()`
  2. `bs58`/`String` cleanup w CPI trade path
  3. `market_cap_sol` dla PumpSwap CPI
  4. TTL dla `ResolveQueue`
  5. dopiero na końcu refaktor side-effectów, ale ostrożnie, żeby nie zepsuć same-tx resolution

Krótko: tak, część tych uwag jest trafna, ale obecnych bloków `base_mint/dev unknown` nie tłumaczy przede wszystkim parser. Najbardziej podejrzany był i nadal jest runtime ordering, a `dev_pubkey unknown` dodatkowo wynika z celowej sanitacji.

Jeśli chcesz, mogę teraz zrobić osobny, mały patch wyłącznie dla parsera:
1. usunąć `to_string()` z `is_candidate_owner`,
2. wywalić `bs58` z CPI trade hot path,
3. policzyć `market_cap_sol` dla `CpiSwapBuy/CpiSwapSell`.