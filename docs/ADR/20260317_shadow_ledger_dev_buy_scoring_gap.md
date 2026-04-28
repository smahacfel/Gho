# ADR: Analiza architektury Shadow Ledgera — Dev Buy scoring i Fee 1%

## Data: 2026-03-17
## Status: Analiza zakończona, plan do zatwierdzenia

### Kontekst

Przeprowadzono głęboką analizę architektoniczną Shadow Ledgera pod kątem trzech hipotez:
1. **Hardkodowanie `dev_buy_sol: 0.0`** w `oracle_pipeline.rs` przy budowaniu `EnhancedCandidate`
2. **Brak uwzględniania fee 1% Pump.fun** w aktualizacjach stanu krzywej CPMM
3. **Brak natychmiastowej aplikacji dev buy** jako pierwszego trade'a na krzywej

### Ustalenia

#### 1. POTWIERDZONE (BUG SCORINGU): `dev_buy_sol: 0.0` w `oracle_pipeline.rs`

Plik `oracle_pipeline.rs`, linia 697 w `convert_to_enhanced_candidate()`:
- `dev_buy_sol: 0.0` — hardkodowane
- `has_dev_buy: false` — hardkodowane

To oznacza, że **wszystkie systemy scoringowe** (HyperPrediction, SurvivorScore, build_dev_buy_wave, dev_buy_wave_scaled) otrzymują fałszywe dane — traktują każdego kandydata jakby dev nie zrobił żadnego zakupu. Wpływa to na jakość sygnałów Oracle, ale **NIE** na stan krzywej w pamięci.

Przyczyną jest architekturalna separacja — `DetectedPool` (emitowany na Pool Detected) nie zawiera informacji o dev buy, ponieważ dev buy to oddzielny `TradeEvent` w IPC, nawet jeśli pochodzi z tej samej atomowej transakcji.

#### 2. FALSE ALARM: Fee 1% w krzywej jest poprawne

Analiza `apply_trade_strict()` w `history_types.rs` wykazała:
- **BUY**: `sol_after_fee = d_sol_lamports * 99 / 100` → netto SOL wpada do krzywej
- **SELL**: `d_sol_before_fee * 99 / 100` → netto SOL jest wypłacane
- Stała `k` jest zachowywana (k = R_sol × R_tok obliczane raz z genesis state)

Ścieżki korzystające z poprawnej matematyki:
- `GatekeeperMintBuffer.add_tx()` → `apply_trade_strict()`
- `LivePipeline.flush()` → `apply_trade_strict()`
- `forward_simulation.rs` → `sol_after_fee = sol_lamports * 99 / 100`
- `simulation.rs` → `FEE_BPS = 100` (1% fee)

#### 3. FALSE ALARM: Dev buy jest poprawnie aplikowany do krzywej

Gatekeeper poprawnie akumuluje **każdy** trade (w tym dev buy) w `BufferedTx` i aplikuje go do `ReconstructedState` przez `apply_trade_strict()` (gatekeeper.rs:292). Test `test_dev_buy_first_tx()` potwierdza to. LivePipeline aplikuje identycznie przez `apply_trade_strict()` (live_pipeline.rs:667).

#### 4. ODKRYTY BUG (NISKI PRIORYTET): Genesis `real_sol_reserves`

`PROTOCOL_GENESIS_REAL_SOL_RESERVES = 30_000_000_000` (30 SOL) w `genesis.rs`. Według specyfikacji Pump.fun, Real SOL Reserve powinno wynosić 0 przy genesis (Virtual SOL = 30 jest poprawne). Pole `real_sol_reserves` **nie jest używane** w matematyce CPMM Shadow Ledgera (ReconstructedState operuje na virtual reserves), ale może wpływać na `bonding_progress` computation.

### Decyzje Architektoniczne

1. **Priorytet 1**: Naprawić hardkodowanie w `oracle_pipeline.rs` przez rozszerzenie `DetectedPool` o pola `dev_buy_sol` i `has_dev_buy`, wypełniane z atomowego dev buy TradeEvent
2. **Priorytet 2**: Skorygować `PROTOCOL_GENESIS_REAL_SOL_RESERVES` na 0 (po weryfikacji downstream dependencies)
3. **Brak zmian**: Matematyka CPMM i fee 1% — potwierdzone jako poprawne, brak interwencji

### Konsekwencje i Ryzyka

- **Ryzyko regresji scoringu (Średnie)**: Zmiana `dev_buy_sol` z hardkodowanego 0.0 na prawdziwą wartość zmieni wyniki scoringu Oracle — testy `ghost-brain` (HyperPrediction, scoring.rs) mogą wymagać aktualizacji fixtures
- **Brak zaburzenia SSOT krzywej**: Matematyka CPMM jest poprawna, fee jest uwzględniane, dev buy jest aplikowany. Zmiana dotyczy wyłącznie warstwy scoringowej Oracle
- **Kontrakty nienaruszone**: `apply_trade_strict()`, `k` invariant, fee deduction — wszystko poprawne
- **Oś czasu nienaruszona**: Zmiana w scoringu nie wymaga migracji danych ani modyfikacji historii
