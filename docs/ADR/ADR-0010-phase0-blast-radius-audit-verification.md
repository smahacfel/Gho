# ADR-0010: Phase 0 Blast Radius Audit Verification

**Date:** 2026-03-20  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Zweryfikowano dokument `PLANS/FAZA0_ZAMROZENIE_KONTRAKTOW_I_BLAST_RADIUS_20260320.md` względem planu wykonawczego `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md` oraz aktualnego baseline `main@567bc6005b5907b116987339a9a82289759ceae9`.

Celem audytu było potwierdzenie, czy raport Fazy 0:
- zamraża wymagane kontrakty publiczne,
- zawiera kompletny prod writer matrix `ShadowLedger`,
- rozdziela live runtime od startup/replay,
- poprawnie opisuje aktywne emitery i inventory `WalRecord`,
- nazywa najważniejsze rozjazdy między kodem a dokumentacją.

## Decision

Uznano, że raport Fazy 0 jest **merytorycznie silny** i po korekcie audytowej z 2026-03-20 może być traktowany jako **kompletny w jawnie zdefiniowanym zakresie**.

Potwierdzone zostały następujące kluczowe tezy raportu:
- istnieje realny podwójny bootstrap `genesis_curve()`:
  - `off-chain/components/seer/src/lib.rs` seeduje curve przy `Create`,
  - `ghost-launcher/src/main.rs` seeduje ponownie po `GhostEvent::NewPoolDetected`;
- `AccountUpdate` ma aktywną ścieżkę direct-write poza `tx_only`, plus forwarding do repair/reconciliation path;
- launcher startup nie wywołuje `ShadowLedger::restore_from_disk()` ani `replay_shared_wal()`;
- aktywne prod emitery WAL na baseline to tylko `RawTx`, `ParsedEvent` i `Decision`;
- `InitPoolEvent` jest wołany bezpośrednio z bridge'a launcherowego, a nie wyłącznie przez `SnapshotListener`.

Audyt wykrył początkowo jedną lukę kompletności:
- report nie uwzględniał aktywnego mutującego callsite'u `ShadowLedger.register_curve_alias(...)` w `ghost-launcher/src/oracle_runtime.rs` podczas rejestracji canonical runtime pool identity.

Luka została domknięta przez aktualizację raportu Fazy 0:
- doprecyzowano scope writer matrixa do writerów curve/history,
- dodano osobną sekcję inventory dla alias-only mutacji `ShadowLedger`,
- skorygowano checklistę wyjściową, aby nie mieszała writerów precedence z metadata-only mutations.

W konsekwencji raport może być dalej używany jako zamrożony punkt wejścia do Fazy 1 bez ukrytej luki semantycznej w inventory storage layer.

## Architectural Impact

ADR ustala, że Phase 0 inventory jest wiarygodną bazą wejściową do Fazy 1, pod warunkiem utrzymania jawnego rozróżnienia między:
- writerami curve/history,
- alias-only mutatorami metadata storage.

Wpływ na system:
- Faza 1 może bezpiecznie używać obecnego raportu do pracy nad precedence, bootstrap semantics i recovery ordering.
- Każdy kolejny dokument lub ADR odwołujący się do `complete writer matrix` musi uwzględniać różnicę między:
  - writerem curve/history,
  - alias-only mutatorem storage metadata.

## Risk Assessment

**Rate: Low**

Ryzyko regresji produkcyjnej jest niskie, bo ADR i korekta raportu nie zmieniają kodu runtime. Pozostaje jedynie ryzyko dyscypliny dokumentacyjnej:
- przyszłe dokumenty nie mogą ponownie mieszać alias-only mutations z precedence writerami bez jawnego nazwania zakresu,
- nowe storage mutation pathy muszą być dopisywane do właściwej sekcji inventory.

## Consequences

Co staje się łatwiejsze:
- można dalej używać raportu Fazy 0 jako praktycznie poprawnego SSOT dla głównych pathów,
- znane krytyczne tezy raportu mają niezależne potwierdzenie w kodzie,
- plan Fazy 1 nie wymaga restartu od zera,
- blast radius storage layer obejmuje już także alias-only mutation pathy.

Co staje się trudniejsze:
- trzeba utrzymać konsekwentny podział między precedence plane i metadata-only plane,
- nie wolno wrócić do niejawnej definicji `writera ShadowLedger` w kolejnych fazach.

## Alternatives Considered

### 1. Uznać raport za w pełni kompletny bez zastrzeżeń
Odrzucono dla wersji pierwotnej raportu, bo `ghost-launcher/src/oracle_runtime.rs` zawiera realny aktywny alias-write do `ShadowLedger`, który nie był wpisany ani do matrixa, ani do osobnego inventory.

### 2. Odrzucić raport jako nierzetelny
Odrzucono, bo większość tez raportu została potwierdzona bezpośrednio w kodzie, a luka ma charakter punktowy, nie systemowy.

### 3. Traktować alias-only writes jako poza zakresem bez dopisku w raporcie
Odrzucono, bo takie podejście ponownie tworzyłoby ukryty zakres i psuło closure-mode invariant jawnej inwentaryzacji.

### 4. Skorygować raport przez rozdzielenie writer matrixa i alias inventory
Przyjęto, bo to najmniejsza poprawna zmiana: nie fałszuje planu Fazy 1, nie rozszerza scope'u, a usuwa jedyną wykrytą lukę kompletności.

## Validation Steps

Audyt i korekta zostały zweryfikowane przez:
1. potwierdzenie baseline repo: `HEAD == 567bc6005b5907b116987339a9a82289759ceae9`,
2. sprawdzenie jedynej zmiany w repo: nowy plik `PLANS/FAZA0_ZAMROZENIE_KONTRAKTOW_I_BLAST_RADIUS_20260320.md`,
3. inspekcję aktywnych callsite'ów dla:
   - bootstrapów `genesis_curve()` i `store_curve_with_snapshots()`,
   - `handle_account_update()`,
   - `seed_curve_via_rpc()`,
   - `commit_history()` i `append_live()`,
   - `restore_committed_history_from_wal()` i `replay_shared_wal()`,
   - enum `WalRecord` i prod append-site'ów WAL,
4. sprawdzenie, że `restore_from_disk()` i `replay_shared_wal()` nie są wywoływane z `ghost-launcher/src/main.rs`,
5. sprawdzenie, że dodatkowe `commit_history()` znalezione w `ghost-brain` należą do bloków testowych,
6. wykrycie nieujętego alias-write pathu w `ghost-launcher/src/oracle_runtime.rs`,
7. aktualizację raportu Fazy 0 o osobną sekcję alias-only mutation inventory,
8. korektę checklisty i scope statement tak, aby odpowiadały rzeczywistej definicji inventory.
