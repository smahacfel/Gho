# FSC UNLOCK — FINAL EXECUTION PLAN

**Date:** 2026-04-17  
**Status:** Final  
**Owner:** Ghost Father

## 0. Cel dokumentu

Ten dokument zawęża wcześniejszy plan `authoritative FSC funding lane` do **czterech niezależnie merge'owalnych PR-ów**.

Celem nie jest „odpalenie FSC za wszelką cenę”, tylko doprowadzenie do stanu, w którym:

1. `full_chain_coverage=true` jest **uczciwe**, a nie wymuszone,
2. `FSC` pozostaje na kanonicznej ścieżce `MaterializedFeatureSet.sybil_resistance`,
3. nie naruszamy SSOT, fail-closed semantics, replay determinism ani istniejących kontraktów transportowych,
4. rollout jest odwracalny po każdym PR-ze,
5. aktywacja policy `FSC` pozostaje **poza zakresem** tego planu i następuje dopiero po bake.

---

## 1. Zasady nienegocjowalne

Poniższe granice obowiązują we wszystkich PR-ach:

1. **Canonical feature path pozostaje SSOT.**
   - `FSC` może trafić do decyzji tylko przez `MaterializedFeatureSet.sybil_resistance.funding_source_concentration`.
   - Policy nie dostaje bezpośredniego dostępu do `FundingSourceIndex`.

2. **Fail-closed semantics pozostają nienaruszone.**
   - `full_chain_coverage=true` wolno ustawić wyłącznie dla eventów z dedykowanego authoritative funding lane.
   - Obecny `grpc_global_stream` nie zmienia znaczenia i nie może zostać „awansowany” do full coverage przez rename albo flip booleana.

3. **Brak RPC hot-path.**
   - `cluster_hunter::trace_funding_source` i podobne ścieżki pozostają offline/diagnostic only.
   - Materialization i runtime nie wykonują sieciowych dogrywek funding history.

4. **Brak pool-local heurystyk dla funding provenance.**
   - Funding provenance pozostaje niezależne od pool attachment i trade buffering.

5. **Backward compatibility transportu jest obowiązkowa.**
   - Zmiany w Seer IPC i launcher events są additive.
   - Stare fixtures/serde surfaces muszą nadal działać.

6. **Aktywacja policy `FSC` nie wchodzi do tych PR-ów.**
   - Ten plan odblokowuje uczciwe coverage i runtime readiness.
   - Progi, kary i combo-veto dla `FSC` pozostają na osobny follow-up po bake.

---

## 2. Strategia dostarczenia

Plan dzieli się na **4 PR-y** o rosnącym blast-radiuse:

| PR | Nazwa | Zmienia zachowanie domyślne? | Rollback | Blokuje kolejne? |
|---|---|---:|---|---:|
| 1 | Contract freeze + funding provenance contract | Nie | bardzo łatwy | Tak |
| 2 | Seer authoritative funding lane (disabled by default) | Nie | łatwy | Tak |
| 3 | Launcher/runtime readiness wiring | Nie przy default config | średni | Tak |
| 4 | Observability + bake package | Nie | łatwy | Nie |

**Zasada dostarczenia:** każdy PR ma być merge'owalny samodzielnie i bezpieczny przy domyślnej konfiguracji. Dopóki authoritative lane nie jest jawnie włączony, system ma zachowywać się dokładnie jak dziś.

---

## 3. PR-1 — Contract freeze + additive funding provenance contract

## 3.1. Cel

Ustalić i zakodować kontrakt, który rozróżnia:

- filtered funding observations,
- authoritative full-feed funding observations,
- provenance / replay semantics potrzebne do audytu,

bez zmiany zachowania runtime.

## 3.2. Exact scope

### In scope

1. **Doprecyzowanie kontraktu funding transportu w Seer IPC**
   - `off-chain/components/seer/src/ipc.rs`
   - utrzymanie `full_chain_coverage: bool` jako stabilnego pola downstream,
   - ewentualne dodanie additive pól typu:
     - `funding_lane_kind` / `coverage_class`,
     - `replay_origin`,
     - innego jawnego provenance marker tylko dla funding transfers.

2. **Doprecyzowanie kontraktu launcher eventu fundingowego**
   - `ghost-launcher/src/events.rs`
   - `ghost-launcher/src/components/seer.rs`
   - propagation nowych pól 1:1, bez zmiany znaczenia istniejących pól.

3. **Zamrożenie semantyki obecnego `grpc_global_stream`**
   - komentarze, docstringi, testy kontraktowe,
   - jednoznaczne potwierdzenie, że to filtered trade lane, a nie full-chain funding feed.

4. **Dokumentacja granic**
   - follow-up/update do `ADR-0096` i `ADR-0101`,
   - ten plan pozostaje dokumentem wykonawczym, ADR ma zamrozić boundary.

5. **Testy backward compatibility**
   - serde old fixture compatibility,
   - filtered funding event nadal deserializuje się poprawnie,
   - brak regressji event-ordering i timestamp provenance.

### Out of scope

- nowy subscribe request,
- nowy lane fundingowy,
- jakakolwiek zmiana `authoritative_funding_stream_available`,
- jakakolwiek zmiana policy,
- globalny rename wszystkich `source_label` do enumów.

## 3.3. Critical files

- `/root/Gho/off-chain/components/seer/src/ipc.rs`
- `/root/Gho/off-chain/components/seer/src/lib.rs`
- `/root/Gho/ghost-launcher/src/events.rs`
- `/root/Gho/ghost-launcher/src/components/seer.rs`
- `/root/Gho/docs/ADR/ADR-0096-phase6-fsc-preflight-and-data-plane-boundaries.md`
- `/root/Gho/docs/ADR/ADR-0101-sybil-degraded-reason-classification-fsc-vs-sfd.md`

## 3.4. Merge criteria

PR-1 może zostać zmergowany tylko jeśli:

1. default behavior pozostaje niezmienione,
2. stare fixtures/serde surfaces nadal przechodzą,
3. `grpc_global_stream` pozostaje semantycznie filtered lane,
4. nie ma żadnej ścieżki, która zaczyna emitować `full_chain_coverage=true`,
5. testy kontraktowe potwierdzają additive compatibility.

## 3.5. Rollback boundary

Rollback PR-1 nie wymaga migracji runtime state ani config rollbacku; to ma być czysta warstwa kontraktowo-dokumentacyjna.

---

## 4. PR-2 — Seer authoritative funding lane (disabled by default)

## 4.1. Cel

Dodać dedykowany full-feed funding lane do Seera, ale tak, żeby:

- obecny trade/pool detection nie zmienił zachowania,
- authoritative lane był jawnie oddzielny od `grpc_global_stream`,
- całość była domyślnie wyłączona.

## 4.2. Exact scope

### In scope

1. **Jawny config funding lane**
   - `off-chain/components/seer/src/config.rs`
   - dodać tryb typu:
     - `disabled`,
     - `pump_filtered`,
     - `full_chain`,
   - default = fail-closed (`disabled` albo równoważny brak authoritative lane).

2. **Osobny subscribe/profile dla authoritative funding lane**
   - `off-chain/components/seer/src/grpc_connection.rs`
   - obecny subscribe request dla trade/pool detection pozostaje bez zmiany znaczenia,
   - nowy funding lane nie współdzieli semantycznie obecnego `grpc_global_stream`.

3. **Rozdzielenie emission logic w Seerze**
   - `off-chain/components/seer/src/lib.rs`
   - filtered lane nigdy nie emituje `full_chain_coverage=true`,
   - authoritative lane może emitować `true`, ale wyłącznie gdy użyty jest właściwy full-feed profile.

4. **Minimalna telemetria lane separation**
   - metrics/logging per lane,
   - rozróżnienie authoritative funding events vs filtered funding events.

5. **Testy lane boundaries**
   - filtered lane → zawsze `false`,
   - authoritative lane → `true` tylko w odpowiednim mode,
   - trade detection/buffering bez regressji.

### Out of scope

- launcher/runtime readiness wiring,
- zastąpienie startup hardcode w `main.rs`,
- policy activation,
- szeroki refaktor stringowego `source_label` w całym Seerze,
- zmiana canonical feature path.

## 4.3. Critical files

- `/root/Gho/off-chain/components/seer/src/config.rs`
- `/root/Gho/off-chain/components/seer/src/grpc_connection.rs`
- `/root/Gho/off-chain/components/seer/src/lib.rs`
- `/root/Gho/off-chain/components/seer/src/types.rs` *(tylko jeśli potrzebne helpery ograniczonego zasięgu)*

## 4.4. Merge criteria

PR-2 może zostać zmergowany tylko jeśli:

1. przy domyślnej konfiguracji zachowanie jest identyczne jak dziś,
2. istniejący filtered trade lane ma niezmieniony blast-radius i semantics,
3. authoritative funding lane jest jawnie wyłączony by default,
4. nie ma ścieżki, w której `grpc_global_stream` zaczyna dawać `full_chain_coverage=true`,
5. testy potwierdzają lane separation i brak regressji w trade detection.

## 4.5. Rollback boundary

Rollback PR-2 ma polegać na wyłączeniu nowego config mode albo cofnięciu osobnego subscribe/profile bez wpływu na stary trade lane.

---

## 5. PR-3 — Launcher/runtime readiness wiring

## 5.1. Cel

Usunąć startupowy hardcode jako jedyne źródło availability i podłączyć launcher/runtime do rzeczywistego stanu authoritative funding lane, bez naruszenia fail-closed.

## 5.2. Exact scope

### In scope

1. **Zastąpienie startupowego hardcode bez flipowania go na ślepo**
   - `ghost-launcher/src/main.rs`
   - `ghost-launcher/src/oracle_runtime.rs`
   - `authoritative_funding_stream_available` przestaje być twardym `false` w kodzie,
   - nowy stan availability pochodzi z config + zdrowia lane'u albo z dedykowanego control-plane signal.

2. **Bezpieczne spięcie z SessionManager / FundingSourceIndex**
   - `ghost-launcher/src/session/manager.rs`
   - `ghost-launcher/src/tx_intelligence/funding_source.rs`
   - zachować obecną własność globalnego bounded indexu,
   - zachować obecną logikę, że tylko authoritative transfer może rozgrzać readiness.

3. **Readiness/warmup semantics bez regresji**
   - authoritative lane disabled/unhealthy → `FSC_FUNDING_STREAM_UNAVAILABLE` / fail-closed,
   - authoritative lane healthy + authoritative transfer observed → możliwy warmup,
   - partial lane nadal nie odblokowuje readiness.

4. **Diagnostyka runtime**
   - jawne logowanie stanu availability/warmup,
   - brak mieszania observability z policy.

5. **Testy runtime/session**
   - default config zero drift,
   - authoritative lane disabled = stare degraded behavior,
   - authoritative lane enabled + synthetic authoritative transfer = readiness przechodzi poprawnie,
   - filtered transfer nie odblokowuje readiness.

### Out of scope

- tuning `FSC` thresholds,
- policy activation albo kary,
- direct reads z `FundingSourceIndex` po stronie policy,
- przenoszenie funding state do sesji.

## 5.3. Critical files

- `/root/Gho/ghost-launcher/src/main.rs`
- `/root/Gho/ghost-launcher/src/oracle_runtime.rs`
- `/root/Gho/ghost-launcher/src/session/manager.rs`
- `/root/Gho/ghost-launcher/src/tx_intelligence/funding_source.rs`
- `/root/Gho/ghost-launcher/src/session/observation.rs`

## 5.4. Merge criteria

PR-3 może zostać zmergowany tylko jeśli:

1. przy authoritative lane disabled nie ma decision drift,
2. runtime nie promuje availability przed dowodem coverage/health,
3. `FundingSourceIndex` pozostaje jedynym stateful source dla FSC lookup,
4. filtered transfer nadal nie odblokowuje readiness,
5. canonical feature path pozostaje jedyną drogą do materialized `FSC`.

## 5.5. Rollback boundary

Rollback PR-3 ma być możliwy przez przywrócenie starego startup behavior albo wyłączenie authoritative lane config, bez naruszania event contractu i bez potrzeby migracji persisted state.

---

## 6. PR-4 — Observability + bake package + rollout guardrails

## 6.1. Cel

Domknąć wszystko, co jest potrzebne do bezpiecznego bake, ale nadal **bez aktywacji kar/policy `FSC`**.

## 6.2. Exact scope

### In scope

1. **Metrics/logging authoritative funding lane**
   - lane health,
   - reconnect/stall counters,
   - emitted authoritative funding events,
   - emitted filtered funding events,
   - `fsc_warmup_ready`, lookup hit-rate, prune/eviction telemetry.

2. **Decision / diagnostics surfaces**
   - jawny rozdział między:
     - canonical materialized `FSC`,
     - lane observability,
     - degraded reasons.

3. **Bake/runbook package**
   - `docs` / `PLANS` / rollout notes,
   - checklista bake,
   - warunki wejścia do ewentualnej przyszłej policy activation.

4. **Replay / paper-burnin validation harness**
   - neutral config replay diff,
   - brak driftu przy authoritative lane disabled,
   - expected-only drift przy authoritative lane enabled.

5. **Operator-facing config guidance**
   - które profile włączają authoritative funding lane,
   - które pozostają bezpiecznie neutralne,
   - jak rollbackować bez ruszania SSOT.

### Out of scope

- `soft_penalty_high_fsc`,
- `RejectSybilSoftExcess` zależny od `FSC`,
- combo-veto activation,
- any hot-path RPC fallback.

## 6.3. Critical files

- `/root/Gho/docs/ADR/ADR-0096-phase6-fsc-preflight-and-data-plane-boundaries.md`
- `/root/Gho/docs/ADR/ADR-0101-sybil-degraded-reason-classification-fsc-vs-sfd.md`
- `/root/Gho/docs/RUNBOOK_*` *(według finalnego miejsca rollout docs)*
- `/root/Gho/PLANS/*` *(jeżeli potrzebna checklista bake)*
- pliki metrics/logging w `ghost-launcher` i `off-chain/components/seer`

## 6.4. Merge criteria

PR-4 może zostać zmergowany tylko jeśli:

1. istnieje jasny bake checklist,
2. authoritative lane health i `FSC` readiness są obserwowalne bez grzebania w kodzie,
3. replay diff i paper-burnin workflow są opisane i wykonalne,
4. default production behavior nadal nie aktywuje `FSC` policy,
5. rollback procedura jest jawna i krótka.

## 6.5. Rollback boundary

Rollback PR-4 nie powinien wymagać cofania kodu logicznego z PR1–PR3; ma być w większości rollbackiem dokumentacyjno-obserwacyjnym lub configowym.

---

## 7. Zależności między PR-ami

- **PR-1 blokuje wszystko** — bez zamrożonego kontraktu łatwo rozwalić semantykę coverage.
- **PR-2 zależy od PR-1** — nowy lane musi korzystać z ustalonego provenance contract.
- **PR-3 zależy od PR-2** — launcher/runtime ma sens dopiero, gdy istnieje prawdziwy authoritative lane.
- **PR-4 zależy od PR-2 i PR-3** — nie ma sensu budować bake package dla czegoś, czego nie da się obserwować end-to-end.

---

## 8. Czego ten plan świadomie NIE robi

1. Nie aktywuje `FSC` jako live policy penalty.
2. Nie zmienia znaczenia `grpc_global_stream`.
3. Nie robi globalnego refaktoru wszystkich `source_label` w Seerze.
4. Nie przenosi funding state do sesji, gatekeepera ani logger-only surfaces.
5. Nie dopuszcza RPC fallback jako authoritative source dla `FSC`.
6. Nie łączy unlocku data-plane z tuningiem progów sybil.

---

## 9. Finalny gate przed przyszłą aktywacją policy FSC

Po wykonaniu PR-1…PR-4 można dopiero rozważyć osobny follow-up dla policy `FSC`, ale wyłącznie jeśli jednocześnie spełnione są wszystkie warunki:

1. authoritative funding lane ma stabilne health metrics,
2. `fsc_warmup_ready` osiąga oczekiwany poziom w bake,
3. lookup hit-rate jest wystarczający,
4. neutral funder hygiene jest zweryfikowana,
5. replay diff pokazuje expected-only drift,
6. `FSC_FUNDING_STREAM_UNAVAILABLE` przestaje dominować tam, gdzie authoritative lane jest włączony,
7. operator ma gotową procedurę rollbacku bez naruszania SSOT.

Dopiero wtedy `FSC` może wejść do osobnego follow-upu policy zgodnego z `ADR-0097`.

---

## 10. Jednozdaniowy werdykt wykonawczy

**Najbezpieczniejsze odblokowanie FSC to nie flip booleana, tylko cztery PR-y: najpierw kontrakt, potem nowy lane, potem runtime readiness, na końcu observability i bake — wszystko additive, fail-closed i bez ruszania canonical feature path.**
