# ADR: Wzbogacanie Danych Transakcji o Inner Instructions (CPI)

## Status
Zaimplementowano (Marzec 2026)

## Kontekst
W logu z systemu produkcyjnego (plik `system.log.2026-03-17`) zauważono bardzo dużą anomalię w logach enrichmentu `ENRICH_RESULT`. Mianowicie:
* `buy_variant=Some(...)` występował tylko w 820 logach
* `buy_variant=None` występował w 17 838 logach (ok 96% populacji typów BUY)

Szczegółowa analiza komponentu `binary_parser.rs` wykazała root cause. Proces tworzenia obiektu `TradeEvent` oraz jego późniejsze wzbogacanie (`enrich_trade_optional_accounts_from_source_ix`) pobierały opcjonalne metadane bezpośrednio z instrukcji top-level (`instructions`). 

W realnych warunkach on-chain, ogromna większość trafiku dla Pump.fun przechodzi przez agregatory i terminale (Oxbull, Jupiter, itp.), gdzie rzeczywista instrukcja BUY leży zagnieżdżona w `inner_instructions` (CPI). Brak odpowiedniego trawersowania tablicy CPI podczas wzbogacania powodował powszechne występowanie pustych własności `buy_variant`, `fee_recipient`, `token_program` i `associated_bonding_curve`.

## Decyzja
Zdecydowano o rozszerzeniu logiki wzbogacania o pełne iterowanie po `inner_instructions`. 

Wprowadzono helper `fill_trade_from_ix_accounts`, który deduplikuje logikę ekstrakcji pól. Obecnie enrichment szuka danych w obu płaszczyznach wykonania:
1. **Faza 1:** Przegląd wszystkich top-level instrukcji `instructions`.
2. **Faza 2:** Jeżeli faza pierwsza nie zebrała całości opcjonalnych danych, silnik trawersuje pętlą wewnętrzne CPI calle w `inner_instructions`.

Testy potwierdzają bezkolizyjność obu trybów i priorytetowanie instrukcji Top-Level nad CPI w przypadku niejednoznaczności. Zmiany obejmują jedynie wypełnianie opcjonalnych pól modelu `TradeEvent` – system SSOT i semantyka parsowania kluczowych stałych wolumenu i logiki stanu nie uległy zmianie, tym samym zagwarantowano brak szkodliwych skutków ubocznych w systemach nadrzędnych. 

## Konsekwencje i wnioski techniczne

* **Wpływ na infrastrukturę**: Oczekiwany jest znaczący wzrost wzbogaconych danych dot. rodzajów zakupu (np. router_exact_sol_in vs legacy_buy) i wskaźników fee, co poprawi analitykę on-chain.
* **Przyspieszenie prac nad heurystyką**: Większa skuteczność pokrywania typów handlu precyzyjniej wytypuje intencje traderów/botów na sieci.
* **Zgodność z SSOT**: Aktualizacja tylko dla opcjonalnych `Option<T>` pól instancji `TradeEvent` w 100% zachowuje stabilność pierwotnych odczytów ze strumieni geyser.
