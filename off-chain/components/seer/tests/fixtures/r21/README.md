# R21: rzeczywiste instrukcje w otoczce testowej

Instrukcje, logi i salda pochodzą z udanych getTransaction z diagnozy r21. To nie są oryginalne ramki gRPC. Pola provider, provenance, hash [1;32], monotonic time i tx_index=0 są jawnie syntetycznymi danymi testowymi; nie stanowią dowodu authority ani kolejności on-chain. Testy są offline i nie uruchamiają RPC.

```json
[
  {
    "fixture": "create_buy_v2",
    "signature": "33c4dUcbAbCNcnyHkeWgEa2b3c5ayzPPiUete8ch8rWC2xzrveKrz7huvBfLGaZiwamyS9AJsFRYFbJkRcoSLvM4",
    "slot": 449392149,
    "original_rpc_sha256": "43a81e2dd0d7c2883a94482e402492317cd112d2b2ea2eb39f6486deaa59dfc7"
  },
  {
    "fixture": "sell_v2",
    "signature": "4G9vkFF993QyYeSgfzuSYK4DYEE9VSMwroJqDYv6un1uV2yXusRbb8wssRMykGuAzzPhBLQVp9gUczdTJMQDf7W9",
    "slot": 449392494,
    "original_rpc_sha256": "29a50255cff0be96abe077a324ca2e03a21cf15a38be94ae0605c4bf035d309c"
  }
]
```

Oficjalny układ kont i discriminators: `pump_trade_idl_excerpt.json`, z adresem źródła, datą pobrania i SHA-256 całego IDL. Wyciąg jest dowodem kontraktu ABI, nie źródłem authority dla transakcji.
