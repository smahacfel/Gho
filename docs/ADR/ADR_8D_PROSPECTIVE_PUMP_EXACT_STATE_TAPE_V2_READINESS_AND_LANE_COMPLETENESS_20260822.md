# ADR-8D: Prospective Pump Exact-State Tape V2 — bootstrap/source overlap i wymagane lane'y

**Data:** 2026-08-22
**Status:** REVISED LOCALLY / INDEPENDENT CODE REVIEW PASS / FINAL CAPTURE NOT STARTED / QUALIFIER AUTHORITY DOCUMENTED SEPARATELY / ALLOWLIST-ONLY COMMIT AUTHORIZED
**Typ:** ADR-8D / prospective raw-capture integrity / fail-closed recorder correction

## D0. Powód korekty

Pierwotny lokalny szkic V2 ustanawiał stream, następnie pobierał finalized
`getProgramAccounts(Pump)` i używał context slotu snapshotu jako początku
prospektywnej kohorty. Sama kolejność operacji nie dowodziła jednak, że każdy
wymagany Yellowstone lane zaczął dostarczać evidence nie później niż ten
snapshot. Ponadto writer mógł technicznie zamknąć raw segment, mimo że provider
nie dostarczył któregoś wymaganego rodzaju wiadomości.

To nie dotyczy frozen GO-D, V1, provider audit GO-E, Gatekeepera ani runtime'u
Ghost. Jest to wyłącznie korekta przyszłego, oddzielnego V2 recordera.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

## D1. Nowa granica source readiness

Ingress zapamiętuje pierwszy **admitted** slot każdego obowiązkowego lane:

```text
transaction
Pump-owned account update
Slot update
BlockMeta
unfiltered full block
```

Po przyjęciu wszystkich pięciu lanes recorder tworzy:

```text
source_readiness_slot = max(first_transaction_slot,
                            first_account_update_slot,
                            first_slot_update_slot,
                            first_block_meta_slot,
                            first_full_block_slot)
```

Finalized GPA snapshot jest dopuszczalny wyłącznie gdy:

```text
finalized_context_slot >= source_readiness_slot
```

Jeżeli finalized provider jeszcze laguje, recorder wykonuje następny GPA request
wyłącznie w pozostałym budżecie istniejącego `bootstrap_rpc_timeout_ms`. Nie ma
drugiego, nieograniczonego retry budgetu. Brak wszystkich lane'ów lub brak
overlapu kończy bootstrap fail-closed i nie tworzy `Complete`.

Readiness raw record oraz completion receipt zapisują pierwsze sloty lane,
`source_readiness_slot`, finalized context slot, liczbę snapshot attempts i
literalny wynik overlapu. Offline consumer nie musi odtwarzać ukrytej logiki
control plane z logu procesu.

## D2. Writer-owned census i full-block reconciliation

Writer liczy lane dopiero po zdekodowaniu i trwałym zapisaniu source evidence.
Pełne bloki przechodzą dodatkową strukturalną reconciliation:

```text
FullBlockPayloadStarted
  -> exactly ordered chunks
  -> exact byte count
  -> SHA-256 + BLAKE3 match
  -> FullBlockPayloadCompleted
```

Normalne zamknięcie publicznego V2 capture wymaga jednocześnie:

```text
transaction_messages > 0
account_updates > 0
slot_updates > 0
block_meta_updates > 0
full_blocks_started > 0
full_blocks_started == full_blocks_completed
incomplete_full_block_payloads == 0
unbound_full_block_chunks == 0
full_block_payloads_reconciled == true
```

Jeżeli brakuje jedynie wymaganego lane przy nadal poprawnej strukturalnie
reconciliation, writer publikuje evidence z terminalnym
`clean_shutdown=false`, a completion receipt jest `Incomplete`. Jeżeli sama
reconciliation full-block narusza start/chunk/completion/digest contract,
writer kończy się błędem fail-closed (może pozostać wyłącznie `.partial`) i
również nie może utworzyć `Complete`. Żaden semantycznie cichy provider ani
częściowy subscription branch nie może otrzymać statusu raw `Complete`.

Ten census jest warunkiem koniecznym, lecz nie samodzielnym dowodem
per-slot completeness. `ADR_8D_PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_P0_EVIDENCE_FRONTIER_AND_EVENT_CPI_20260823.md`
dodaje późniejszy, obowiązkowy offline contract: każdy BlockMeta/FullBlock w
akceptowanej kohorcie musi mieć dokładnie jednego partnera z tym samym
slot/parent/hash/count, a forward availability może pochodzić wyłącznie z
ostatniej takiej pary. Sama równość dwóch Pump transaction maps ani późny Slot
nie są wystarczające.

## D3. Zakres i brak operacji

Korekta nie:

- nie uruchamia preflightu ani capture'u V2;
- nie łączy się z RPC, Yellowstone ani GO-E;
- nie usuwa `/tmp` diagnostics;
- nie zmienia GO-D raw V1, jego hashy ani artefaktów;
- w chwili tej korekty nie implementowała jeszcze V2 materializera ani
  qualifiera; ten historyczny punkt zastępuje wyłącznie
  `ADR_8D_PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_QUALIFICATION_AUTHORITY_20260822.md`;
- nadal nie implementuje exportu lub strategii;
- nie zmienia aktywnego Ghost runtime'u, Gatekeepera ani execution.

V2 nadal nie jest Research Tape gotową do strategii. Po tej korekcie jest tylko
bezpieczniejszym local draftem raw recordera. Finalny capture wymaga oddzielnego
review, clean commit/preflight, prywatnego operator configu, wystarczającego
storage i późniejszego V2 materializer/qualification path.

## D4. Lokalne regresje

Dodane są testy dla:

1. brakującego full-block lane — brak clean completion oraz typed census error;
2. wszystkich lane i `source_readiness_slot == max(first_lane_slots)`;
3. GPA snapshotu starszego od readiness — fail-closed przed persistence;
4. lokalnego loopback retry: stale `S=104`, potem overlap `S=105`, dwa
   snapshot attempts w jednym bootstrap budget;
5. pełnego, poprawnie zamkniętego full-block payloadu;
6. rozpoczętego, lecz niedomkniętego full-block payloadu — brak clean footer.

Pełna macierz i niezależny self-review są wymagane przed zmianą statusu tego
ADR na review-passed. Brak lokalnego szablonu `docs/ADR/ADR_8D_SZABLON.md`
został sprawdzony; dokument zachowuje używany w tym checkoutcie format ADR-8D.

## D5. Rollback

Rollback polega wyłącznie na niewykonywaniu V2 preflight/capture. Nie wymaga
resetu, usuwania ani reinterpretacji GO-D, a V2 nie posiada istniejącego raw
runu do wycofania.
