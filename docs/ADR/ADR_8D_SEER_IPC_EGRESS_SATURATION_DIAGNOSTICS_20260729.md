# ADR-8D — Seer IPC egress: diagnostyka saturacji bez zmiany sterowania

**Data:** 2026-07-29
**Status:** IMPLEMENTED / DIAGNOSTIC-RUN-PENDING / DAY1-BLOCKED
**Zakres:** `off-chain/components/seer/src/ipc.rs` oraz addytywna semantyka
`LocalGapTracker` w `off-chain/components/seer/src/local_gap.rs`

## D0. Decyzja

Dodano wyłącznie dwa markery diagnostyczne dla Seer IPC egress:

```text
IPC_EGRESS_SATURATED
IPC_DOWNSTREAM_FULL
```

Pierwszy jest emitowany raz na ciągły epizod saturacji egress i zapisuje:

- `event_kind` oraz `provider_id`;
- `normal_len` / `normal_capacity`;
- `account_update_len` / `account_update_capacity`;
- `downstream_remaining_capacity` / `downstream_max_capacity`;
- `events_sent` / `events_received`;
- `timestamp_unix_ms`.

`IPC_DOWNSTREAM_FULL` jest emitowany przez istniejący dispatcher przy pierwszym
pełnym downstream oraz potem najwyżej raz na sekundę. Zapisuje
`pending_event_kind` i `continuous_full_duration_ms`.

## D1. Powód

Smoke diagnostyczny z `2026-07-29T19:25:23.600Z` udowodnił pełny normalny
egress lane (`IpcError::LocalProcessingGap`) i po 29 ms pierwsze zamknięcie
admission jako `primary_local_coverage_gap`. Nie zapisał jednak historycznych
wartości `seer_ipc_events_sent_total`, `seer_ipc_events_received_total`,
lane-specific occupancy ani stanu downstream w chwili saturacji.

W szczególności log ostrzegający o wysokiej zajętości nie wystąpił. Efektywny
normal lane ma pojemność 10 000, a metryka ostrzegawcza dzieli sumaryczną
zajętość przez normal lane + AccountUpdate lane (42 768), zatem pełny normal
lane odpowiada tylko około 23,4% tej agregowanej pojemności i nie osiąga progu
80%.

Bez tej obserwowalności nie można rozróżnić krótkiego burstu producenta od
zatrzymania downstream consumer albo head-of-line blocking. Nie zmienia się
zatem żadna pojemność, kolejka, policy backpressure ani fail-close.

## D2. Zachowane zachowanie

- `IpcEgressQueue::try_enqueue` zachowuje ten sam warunek pełności,
  kolejność sequence i `IpcError::LocalProcessingGap`.
- `LocalGapTracker` nadal otwiera dokładnie ten sam gap; zwracana wartość
  bool mówi jedynie, czy wywołanie utworzyło nowy epizod, aby marker nie był
  wielokrotnie emitowany.
- Fixed dispatcher nadal używa tego samego `try_send`, pending event oraz
  1 ms sleep przy downstream Full. Marker nie wysyła, nie odbiera i nie
  czeka na żadnym kanale.
- Nie zmieniono CandidateIntegrity, ACE probe'a, Brain, Gatekeepera,
  EventWritera, Triggera, configu ani PR2.

## D3. Focused proof i następny krok

Focused testy potwierdzają:

1. snapshot pełnego normal lane zawiera dokładne długości i pojemności obu
   lane'ów;
2. rate-limit downstream Full emituje pierwszy marker, tłumi okres krótszy
   niż 1 s i emituje ponownie po 1 s;
3. pierwszy `LocalGapTracker::observe_saturation` tworzy epizod, a następny
   w tym samym epizodzie go nie tworzy.

Dozwolony jest jeden świeży run diagnostyczny najwyżej 10 minut albo do
pierwszej saturacji. Jego wynik służy wyłącznie do klasyfikacji mechanizmu;
nie jest qualifying smoke'em ani Day 1.

> Uwaga o szablonie: ścieżka wymieniona w instrukcji globalnej,
> `/Gho/docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie.
> Dokument zachowuje lokalny format ADR-8D z `docs/ADR/`.
