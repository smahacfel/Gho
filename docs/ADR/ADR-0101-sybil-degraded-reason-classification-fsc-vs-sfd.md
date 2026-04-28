# ADR-0101: Sybil degraded-reason classification for FSC and SFD

**Date:** 2026-04-15
**Status:** Accepted
**Author:** Ghost Father

## Context

W trakcie walidacji warstwy sybil dla `ghost-launcher` pojawiło się pytanie operacyjne, czy obserwowane degraded reasons:

- `FSC_FUNDING_STREAM_UNAVAILABLE`
- `SFD_POSTBALANCE_UNAVAILABLE`

są poprawnym zachowaniem fail-closed, czy też oznaczają regresję / błąd implementacyjny.

Analiza objęła:

- logi runtime (`gatekeeper_v2_buys.jsonl`, `gatekeeper_v2_decisions.jsonl`),
- źródła degradacji w `ghost-launcher/src/tx_intelligence/funding_source.rs`,
- źródła degradacji w `ghost-launcher/src/tx_intelligence/sybil_metrics.rs`,
- konsumpcję tych degradacji w `ghost-launcher/src/components/gatekeeper_policy.rs`.

Ustalono, że obie degradacje są realnie emitowane w aktualnym runtime, ale nie mają tego samego znaczenia architektonicznego.

## Decision

Przyjmujemy następującą klasyfikację jako SSOT dla obecnego systemu:

1. **`FSC_FUNDING_STREAM_UNAVAILABLE` jest poprawnym, uczciwym zachowaniem fail-closed.**
   - Jest emitowane wtedy, gdy authoritative funding stream nie jest dostępny.
   - Oznacza to, że `FSC` nie ma prawa udawać sygnału gotowego do użycia.
   - To nie jest nowy bug semantyczny w samym `FSC`; to jest uczciwy objaw tego, że upstream funding-plane nie dostarcza jeszcze wymaganej pełnej, autorytatywnej obserwacji.

2. **`SFD_POSTBALANCE_UNAVAILABLE` jest poprawnym fallbackiem fail-closed, ale wskazuje na realną dziurę telemetryczną / niedomkniętą instrumentację.**
   - Jest emitowane, gdy brakuje wymaganych snapshotów balansu signera potrzebnych do policzenia `SFD`.
   - Nazwa reason-code jest lekko myląca, bo w praktyce branch obejmuje brak `post_balance`, ale także brak `pre_balance`.
   - Sam fallback jest poprawny: lepiej zdegradować metrykę niż zmyślić wejście.
   - Jednak częste występowanie tej degradacji nie jest zdrowym steady-state i należy je traktować jako problem data coverage / telemetry.

3. **Obie degradacje pozostają non-actionable w gatekeeper policy.**
   - Zdegradowane `FSC` i `SFD` nie mogą być traktowane jak ważne pozytywne ani negatywne sygnały sybil.
   - System ma degradować się do „nie wiem”, a nie do „wiem i karzę”.

## Architectural Impact

- Warstwa sybil zachowuje fail-closed semantics zamiast tworzyć fałszywie autorytatywne wyniki.
- `FSC` jest jawnie zależne od authoritative funding streamu; brak streamu nie może być maskowany sztucznym wynikiem.
- `SFD` jest jawnie zależne od signer balance snapshots; brak tych danych nie może być maskowany syntetycznym wyliczeniem.
- Policy layer poprawnie odróżnia sygnał dostępny od sygnału zdegradowanego i nie robi z degradacji ukrytej kary.

## Risk Assessment

**Rate:** Medium

- Niskie ryzyko regresji semantyki policy, bo system już fail-closes zamiast udawać gotowość.
- Średnie ryzyko operacyjne, jeśli zespoły błędnie zinterpretują `FSC_FUNDING_STREAM_UNAVAILABLE` jako „bug po fixie”, zamiast jako uczciwy objaw braku authoritative streamu.
- Średnie ryzyko jakości sygnału, jeśli `SFD_POSTBALANCE_UNAVAILABLE` będzie traktowane jako „to tak ma być”, bo w steady-state nie powinno dominować próbki.

## Consequences

- Po fixie `FSC` system może częściej mówić „nie mam autorytatywnego streamu”, zamiast generować pozornie sensowny wynik. To jest pożądane.
- `FSC_FUNDING_STREAM_UNAVAILABLE` należy interpretować jako brak gotowości upstream architektury fundingowej, nie jako nową wadę fail-closed logiki.
- `SFD_POSTBALANCE_UNAVAILABLE` należy interpretować jako brak wymaganych danych wejściowych. Fallback jest poprawny, ale sam upstream gap wymaga osobnego domknięcia.
- Operacyjnie: nie należy tunować progów sybil wokół zdegradowanych metryk, dopóki nie zostanie poprawione źródło danych.

## Alternatives Considered

### 1. Traktować oba reason-codes jako czyste bugi implementacyjne

Odrzucono.

To byłoby nieprawdziwe:
- `FSC_FUNDING_STREAM_UNAVAILABLE` jest celowo emitowany przez branch sprawdzający brak authoritative streamu.
- `SFD_POSTBALANCE_UNAVAILABLE` jest celowo emitowany przy braku wymaganych balance snapshotów.

### 2. Traktować oba reason-codes jako zdrowy steady-state

Odrzucono.

To byłoby równie błędne:
- dla `FSC` reason-code jest poprawnym objawem niedomkniętej gotowości upstream architecture,
- dla `SFD` częste występowanie reason-code oznacza realny data gap, a nie docelowy stan jakości.

### 3. Karać zdegradowane metryki jak normalne sygnały sybil

Odrzucono.

To narusza fail-closed contract i zwiększa ryzyko false positives.

## Follow-up verification after remediation

Po wdrożeniu lokalnej remediacji i ponownej weryfikacji źródeł kodu doprecyzowano granicę między „uczciwie niedostępne” a „częściowo pokryte, ale nadal używalne”:

- **`SFD_POSTBALANCE_UNAVAILABLE` pozostaje poprawną klasyfikacją tylko dla przypadków, w których nie da się zmaterializować wartości `SFD`.**
   - Nie należy go reinterpretować jako ogólnego znacznika „jakakolwiek luka balansu”.
   - Jeśli po odrzuceniu niekompletnych próbek pozostają co najmniej 3 używalne próbki signerów, runtime nie powinien już fail-close całej metryki.

- **`SFD` zostało lokalnie naprawione do trybu partial-but-actionable.**
   - `compute_sfd_from_buys()` wybiera najlepszą dostępną próbkę per signer i materializuje `spend_fraction_divergence`, jeśli pozostaje wystarczająca liczba używalnych próbek.
   - W takim przypadku runtime emituje `SFD_PARTIAL_BALANCE_COVERAGE` zamiast zwijać cały sygnał do `SFD_POSTBALANCE_UNAVAILABLE`.
   - `gatekeeper_policy.rs` traktuje taki liczbowy `SFD` jako actionable; samo `SFD_PARTIAL_BALANCE_COVERAGE` ani `SFD_ZERO_PREBALANCE_SKIPPED` nie wyłącza już sygnału.

- **`FSC` pozostaje architektonicznie zablokowane upstreamem, nie lokalną implementacją.**
    - Ścieżka eventów funding transfer istnieje end-to-end w launcherze i session layer.
    - Seer emituje jednak funding transfery tylko z filtrowanego `grpc_global_stream` i oznacza je `full_chain_coverage: false`.
    - Kontrakt transportowy został dodatkowo zamrożony addytywnym funding provenance contract, który jawnie klasyfikuje ten lane jako filtered i nie pozwala „awansować” go semantycznie samym rename'em albo flipem pola.
    - Dlatego `FSC_FUNDING_STREAM_UNAVAILABLE` pozostaje uczciwym fail-closed symptomem, dopóki nie pojawi się authoritative funding-plane o pełnym pokryciu.

- **PR-1 nie zmienia runtime readiness ani policy semantics.**
   - `full_chain_coverage` pozostaje jedynym stable bitem gotowości dla downstream FSC.
   - Nowe provenance fields są audit/contract only i mają charakter additive/backward-compatible.
   - Filtered funding observations nadal nie mogą odblokować `FSC` warmup ani usunąć `FSC_FUNDING_STREAM_UNAVAILABLE`.

- **Regresja policy została domknięta testami.**
   - Dodano regresje w `ghost-launcher/src/components/gatekeeper_policy.rs`, które pilnują, że częściowo pokryty liczbowy `SFD` pozostaje actionable, a naprawdę nieużywalny `SFD` nadal jest blokowany.
   - Dodatkowo potwierdzono brak regresji w eksporcie telemetrycznym gatekeepera.

## Validation Steps

1. Potwierdzić w źródłach, że:
   - `funding_source.rs` emituje `FSC_FUNDING_STREAM_UNAVAILABLE` przy braku authoritative streamu,
   - `sybil_metrics.rs` emituje `SFD_POSTBALANCE_UNAVAILABLE` przy braku wymaganych balance snapshotów,
   - `gatekeeper_policy.rs` traktuje te degradacje jako non-actionable.
2. Monitorować częstotliwość obu reason-codes w:
   - `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl`,
   - `logs/decisions.jsonl/gatekeeper_v2_decisions.jsonl`.
3. Dla `FSC` osobno śledzić gotowość authoritative funding streamu.
4. Dla `SFD` osobno domknąć balance telemetry coverage i sprawdzić, czy udział `SFD_POSTBALANCE_UNAVAILABLE` spada do akceptowalnego poziomu.
5. Nie zmieniać progów sybil jako obejścia dla zdegradowanych wejść; poprawiać źródło danych, nie symptom.

## PR-4 diagnostics split clarification

Po odblokowaniu authoritative funding lane diagnostyka `FSC` pozostaje celowo rozdzielona na trzy warstwy:

1. **Canonical materialized FSC**
   - `gatekeeper_v2_buys.jsonl -> funding_source_concentration`
   - to jest jedyna kanoniczna wartość `FSC` konsumowana downstream przez policy path.

2. **Degraded reasons**
   - `gatekeeper_v2_buys.jsonl -> sybil_metric_degraded_reasons[]`
   - to jest miejsce, gdzie należy czytać `FSC_FUNDING_STREAM_UNAVAILABLE`, `FSC_ROLLING_STATE_UNAVAILABLE` i inne `FSC_*` fail-closed reasons.

3. **Lane observability**
   - `ghost.pump.*{source_label=...}`,
   - `seer_funding_transfer_observations_total{lane,coverage}`,
   - `fsc_authoritative_funding_stream_available`,
   - `fsc_warmup_ready`,
   - `fsc_lookup_hit_rate`.

Brak lub `None` w `funding_source_concentration` sam w sobie nie rozstrzyga, czy problem leży w lane health, cold warmup czy insufficient known sources. Te stany mają pozostać jawnie rozdzielone, żeby operator nie mylił uczciwego fail-close z regresją policy.
