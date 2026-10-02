# ADR-8D: Gate0 — przekroczenie dev_volume_ratio przez różne sumowanie BUY/SELL

## 1. Zgłoszenie i zakres
Run gate0-1790810736215 zakończył się 2026-10-01 o01:10:27UTC po6291s: PR2B producer invariant failed: manipulation dev_volume_ratio range, exit1, shutdown_error=null. 3170 tokenów,5442snapshoty,15pełnych600s;40sesji przerwanych błędem. Użytkownik zlecił naprawę i osiągnięcie ciągłego runu10h. Transport nie był wskazaną przyczyną tego stopu. Wymieniony globalnie szablon /Gho/docs/ADR/ADR_8D_SZABLON.md nie jest dostępny; zachowano osiem sekcji ADR-8D.

## 2. Role i źródła
SSOT Feature Materialization Guardian, pomocniczo Decision Logging Replay Analyst. Skills ghost-execution i rust-master; przeczytano oba dokumenty specjalistów. Dowody lokalne z JSONL/log/meta oraz aktualnego kodu engine→materializacja→PR2B. Brak zewnętrznego RPC/backfill. Pliki przed zmianą, RED/GREEN i manifest: /tmp/gate0_ratio_repair_20261001.

## 3. Reprodukcja
Rzeczywiste on_transaction: BUY0.89SOL,SELL0.37SOL,BUY0.71SOL tego samego developera. Licznik=(0.89+0.71)+0.37=1.9700000000000002, mianownik=0.89+0.37+0.71=1.97; wynik1.0000000000000002. Test silnika RED. Drugi test przez Gate0 on_trade→tick30s→MFS→PR2B odtworzył dokładnie komunikat stopu, zanim C mogło zakończyć obserwację. Historyczny run nie zachował wadliwej wartości ani kompletu raw wejść; ta reprodukcja dowodzi konkretnego błędu producenta, nie literalnego replay tamtego tokena.

## 4. Przyczyna i luka wcześniejszego testu
Poprzednia poprawka korzystała z tego samego zbioru signerów, lecz dla developera nadal używała interleaved stats.total_volume_sol w mianowniku i sumy BUY+SELL w liczniku. Poprzedni test sprawdzał tylko sprzeczny globalny aggregate. Nie obejmował niełączności f64 wewnątrz tego samego signera.

## 5. Naprawa
Mianownik używa dokładnie tej samej sumy self.dev_buy_volume_total_sol+self.dev_sell_total_sol co licznik compute_dev_behavior, następnie dodaje wolumen pozostałych signerów. Dla skończonych nieujemnych danych nie może być mniejszy niż licznik. Nie ma clampowania, epsilona ani osłabienia kontraktu. Dotyczy wspólnego TxIntelligenceEngine; Gatekeeper policy nie otrzymuje innego źródła metryk. Pozostałe udziały przejrzano, bez spekulacyjnych zmian.

## 6. Diagnostyka i CI
PR2B na ścieżce błędu loguje field,value,value_bits. Gate0 dopisuje mint,phase,cutoff i błąd materializacji, zachowując oryginalny error i fail-closed. JSONL schema/reason pozostają zgodne. Sprawdzono nieudane CI PR101: statyczny test liczył prefiks compute_ftdi_from_buys zarówno w adapterze jak i *_mode. Guard sprawdza teraz dokładne symbole, delegowanie domyślnego profilu i jedną formułę. Nie zmieniono producenta FTDI ani wyłączeń testów. Dalsze sprawdzenie wykazało drugi nieaktualny guard: terminal wywołuje helper materialize_sybil_at_cutoff z pojedynczą gałęzią danego profilu, a nie dawną funkcję bez cutoffu. Guard sprawdza teraz ten faktyczny pojedynczy dispatch i oba wykluczające się profile. Cztery testy lifecycle zachowały dokładne porównanie listy reason codes, uzupełnione o już istniejące CPV_HISTORY_CONFIG_UNAVAILABLE,CPV_SOURCE_CONTINUITY_UNAVAILABLE,CPV_PROGRESS_NOT_AVAILABLE_AT_CUTOFF.

## 7. Walidacja
Dwie regresje RED na starym kodzie. Po poprawce Gate026/26, TxIntelligence185/185 PASS; nowe testy obejmują pełne600s z naprzemiennym BUY/SELL developera,2000zdarzeń z późną identyfikacją oraz niezależną referencję całkowitoliczbową. Test kontraktu wymaga odrzucenia nawet1ULP ponad1, ujemnej wartości, NaN i nieskończoności. Syntetyczne600s nie są dowodem600s realnego strumienia. Końcowe wyniki: PR2A producers28/28 i static guards8/8; PR2B producers23/23 i static guards6/6; policy46/46; session lifecycle39/39; Python7/7; fmt/diff-check PASS; Clippy lib+CLI --no-deps PASS z istniejącymi ostrzeżeniami. Release optimized PASS; finalny hash i manifest w katalogu walidacji. Metadane preflightu i nowego runu są osobnymi dowodami; nie deklarujemy10h PASS przy starcie.

## 8. Self-review i rollout
Przegląd: wspólny subtotal po obu stronach ilorazu, nieznany/znany developer, dodatni wolumen innych signerów, exact-one, długi prefiks i strict guard. C/D/E,Gem320SOL,confirmed,10h admission+do600s drain, źródło i observe-only bez zmian. Sekrety oraz niezwiązane endpointy nietknięte. Zmiana trafia do autoryzowanego PR101; bez merge. Dopiero ukończony realny run może potwierdzić10h stabilności. Udany build/start nie stanowi takiego PASS.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
GO-D nie jest wejściem tej naprawy.
