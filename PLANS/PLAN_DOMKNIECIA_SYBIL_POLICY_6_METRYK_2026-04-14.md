# PLAN DOMKNIĘCIA SYBIL POLICY — 6 METRYK — 2026-04-14

## 0. Cel dokumentu

Ten dokument jest **końcowym planem domknięcia tematu 6 metryk sybil** w `Gatekeeper v2`.

Ma odpowiedzieć jednocześnie na cztery pytania:

1. **jak te metryki mają finalnie wejść do policy**,  
2. **jak zrobić to bez psucia istniejących legacy warstw**,  
3. **jak sprawić, żeby aktywacja naprawdę zmieniała decyzje**, a nie tylko logi,  
4. **w jakiej kolejności operacyjnie to wdrażać**, żeby nie wprowadzić fałszywych rejectów.

Dokument syntetyzuje i zamyka w jednym miejscu ustalenia z:

- `PLANS/PLAN_IMPLEMENTACJI_NOWYCH_METRYK.md`,
- `PLANS/MATRYCA_SYBIL_POLICY_PO_FAZIE5_I_FINAL_2026-04-14.md`,
- `docs/ADR/ADR-0095-sybil-interference-policy-architecture.md`,
- `docs/ADR/ADR-0096-phase6-fsc-preflight-and-data-plane-boundaries.md`.

---

## 1. Stan wyjściowy, który ten plan domyka

Repo jest dziś w stanie **measurement-complete / policy-incomplete**:

1. wszystkie 6 metryk mają kanoniczny nośnik w `MaterializedFeatureSet.sybil_resistance`,
2. `FSC` ma już domkniętą ścieżkę runtime/data-plane i test E2E,
3. buy logi oraz `FINGERPRINT` już mirrorują metryki i degraded reasons,
4. `gatekeeper_policy.rs` nadal liczy tylko **legacy** soft-scoring,
5. aktywny `ghost_brain_config.toml` ma `max_soft_points = 255` oraz `dev_unknown_max_soft_points = 255`, więc obecny soft-scoring jest świadomie „zamrożony”,
6. sybil thresholds / penalties są nadal neutralne, więc obecność pól w JSONL **nie oznacza jeszcze wpływu na BUY/REJECT**.

To jest bardzo ważne: **samo dopisanie sybil points do istniejącego `soft_points` nie jest wystarczające**, jeśli aktywna konfiguracja legacy-soft pozostaje zamrożona na `255`.

Właśnie dlatego ten plan zamyka temat nie tylko przez „jakie flagi liczyć”, ale przez **pełny model aktywacji policy**.

---

## 2. Finalna decyzja architektoniczna

### 2.1. Co robimy

Sześć metryk sybil wchodzi do systemu jako **osobna warstwa `Sybil Interference`**, oparta wyłącznie o kanoniczny snapshot features.

### 2.2. Czego nie robimy

1. **Nie wciskamy tych metryk do legacy phase-thresholds.**
2. **Nie robimy z pojedynczej metryki sybil standalone hard-faila w pierwszym rolloutcie.**
3. **Nie używamy `early_fingerprint` jako authoritative input do BUY/REJECT.**
4. **Nie uzależniamy aktywacji sybil-policy od odmrożenia legacy `max_soft_points`.**

### 2.3. Finalna zasada

Sybil-policy ma być:

- **kanoniczne** — tylko przez `MaterializedFeatureSet.sybil_resistance`,
- **osobne** — nie mieszać semantyki ze starym `SoftSignals`,
- **gotowe do realnego wpływu** — czyli z własnym budżetem punktów / progiem odcięcia,
- **ostrożne** — `None` i degraded metrics zawsze dają `0` kary,
- **interferencyjne** — największą wagę mają kombinacje, nie pojedyncze metryki.

---

## 3. Finalny model policy

## 3.1. Docelowe warstwy decyzji

Docelowy flow powinien wyglądać tak:

1. **Hard Fails** — bez zmian,
2. **Core Pass** — bez zmian,
3. **Legacy Soft Bucket** — bez zmian,
4. **Sybil Interference Bucket** — nowy,
5. **Opcjonalny `RejectSybilInterference` dla whitelistowanych combo-patternów** — dopiero po bake.

W praktyce warstwa 4 i 5 są jedną domeną odpowiedzialności, ale trzeba je logicznie rozdzielić:

- bucket punktowy odpowiada za „miękkie” odcięcie,
- combo-veto odpowiada za bardzo wąski, późny, high-confidence reject.

## 3.2. Dlaczego osobny sybil bucket jest obowiązkowy

Jeżeli sybil points zostaną po prostu dodane do dzisiejszego `soft_points`, to przy aktywnym:

- `max_soft_points = 255`,
- `dev_unknown_max_soft_points = 255`,

nowa logika dalej będzie głównie telemetryczna.

Dlatego finalny model musi dodać **osobny próg**:

- `max_sybil_soft_points`,
- `dev_unknown_max_sybil_soft_points`.

To jest klucz do kompatybilności z resztą systemu:

- **legacy soft-scoring może pozostać zamrożony**, jeśli operator tego chce,
- **sybil-policy może być aktywowana niezależnie**, bez rozwalania obecnych założeń dla starego bucketu.

## 3.3. Finalna kolejność oceny wewnątrz policy

Rekomendowana kolejność w `gatekeeper_policy.rs`:

1. policzyć legacy diagnostics jak dziś,
2. policzyć nowe `SybilPolicyDiagnostics`,
3. jeżeli aktywny jest whitelistowany combo-veto i pattern spełnia warunki gotowości -> `RejectSybilInterference`,
4. w przeciwnym razie, jeżeli `sybil_soft_points > effective_max_sybil_soft_points` -> `RejectSybilSoftExcess`,
5. w przeciwnym razie zachować dotychczasowy BUY/REJECT wynik oparty o hard/core/legacy soft.

### Ważne doprecyzowanie

`total_soft_points = legacy_soft_points + sybil_soft_points` nadal warto eksportować, ale **głównie telemetrycznie**.

Produkcyjny verdict nie powinien w pierwszym kroku zależeć od jednego wspólnego limitu, bo:

- legacy bucket ma inną historię kalibracji,
- sybil bucket ma inną semantykę,
- CPV/FSC mają readiness i warmup,
- aktywny rollout już pokazuje, że legacy max może być celowo „frozen”.

---

## 4. Rola każdej z 6 metryk w finalnym systemie

| Metryka | Finalna rola | Czy może działać solo? | Docelowa siła | Uwagi |
|---|---|---|---|---|
| `DES` | lead signal procesu | tak, jako mocny soft signal | najwyższa | nie robić solo hard-faila w v1 |
| `DBIA` | structural affinity | tylko słabo solo | niska solo / wysoka w combo | sama może oznaczać shared retail infra |
| `FTDI` | structural diversity / counter-signal | tak | średnia | najważniejsza razem z DBIA |
| `SFD` | capital-behavior corroborator | tak | średnio-wysoka | dobrze domyka `DES` |
| `CPV` | rotation corroborator | ostrożnie | średnia | tylko gdy index ready |
| `FSC` | funding corroborator | bardzo ostrożnie | średnia-wysoka | aktywować najpóźniej |

### Jednozdaniowy priorytet

1. **DES prowadzi**,  
2. **`high_dbia && low_ftdi` jest najmocniejszym patternem strukturalnym**,  
3. **SFD potwierdza zachowanie kapitałowe**,  
4. **CPV i FSC wzmacniają case dopiero po warmupie i bake**.

---

## 5. Finalny zestaw sygnałów i patternów

## 5.1. Pojedyncze flagi

Nowy `SybilSoftSignals` powinien zawierać co najmniej:

- `low_ftdi`,
- `high_dbia`,
- `low_sfd`,
- `low_des`,
- `high_cpv`,
- `high_fsc`.

## 5.2. Patterny kombinacyjne obowiązkowe

Nowy `SybilInterferencePattern` / analogiczny enum powinien rozpoznawać co najmniej:

1. `HighDbiaLowFtdi`
2. `LowDesLowSfd`
3. `HighCpvLowDes`
4. `HighFscHighCpv`
5. `HighDbiaLowFtdiLowSfd`
6. `HighFscHighCpvLowDesOrLowSfd`

## 5.3. Patterny, które powinny być traktowane ochronnie

To jest równie ważne jak flagi negatywne:

1. `high_dbia && high_ftdi`  
   - nie traktować jak równoważny pattern cabal,  
   - to może być shared retail bot / wspólna infrastruktura bez nienaturalnej homogeniczności fee topology.

2. `high_cpv` solo  
   - nie dowodzi sybila samo w sobie,  
   - bez `DES` / `SFD` / `FSC` ma mieć tylko lekką wagę albo wyłącznie telemetry.

3. `high_fsc` solo  
   - nie promować do veto,  
   - może być artefaktem niepełnego funding streamu lub złej klasyfikacji neutral funderów.

---

## 6. Ready / degraded semantics — zasady nienegocjowalne

1. **Każda metryka `None` daje zero kary.**
2. **Każda metryka z degraded reason daje zero kary, jeśli degraded state podważa jej wiarygodność produkcyjną.**
3. **CPV i FSC nie mogą penalizować bez readiness / warmupu indeksu.**
4. **FSC nie może penalizować przy `FSC_FUNDING_STREAM_UNAVAILABLE`.**
5. **FSC nie może być combo-inputem do veto, jeśli ma tylko częściową gotowość lub `FSC_INSUFFICIENT_KNOWN_SOURCES`.**
6. **Sybil combo-veto nigdy nie może opierać się na metryce „brak danych = domniemanie winy”.**

W praktyce policy ma traktować sybil snapshot tak:

- `Some(value)` + brak krytycznego degraded state -> metryka może wejść do flagowania,
- `None` lub krytyczny degraded state -> metryka pozostaje wyłącznie obserwowana, bez kary.

---

## 7. Kompatybilność z resztą metryk i obecnym Gatekeeperem

## 7.1. Co zostaje nietknięte

Bez zmian mają pozostać:

- `HardFailReason`,
- core pass logic,
- legacy `SoftSignals`,
- dotychczasowe phase gates,
- dotychczasowe thresholdy viability / safety.

## 7.2. Co dokładamy obok

Dokładamy osobno:

- `SybilSoftSignals`,
- `SybilPolicyDiagnostics`,
- `sybil_soft_points`,
- `effective_max_sybil_soft_points`,
- `sybil_interference_patterns`,
- opcjonalny `sybil_meta_score`,
- nowe verdict types dla sybil-layer.

## 7.3. Co to daje operacyjnie

Ten model jest kompatybilny z resztą repo, bo:

1. nie przepina semantyki istniejących faz,
2. nie zmusza operatora do odmrażania starego `max_soft_points`,
3. nie zmienia starego meaningu `SoftSignals`,
4. pozwala aktywować sybil-policy stopniowo i niezależnie,
5. zachowuje explainability w logach.

---

## 8. Wymagane zmiany w kodzie

| Obszar | Plik / moduł | Zmiana końcowa |
|---|---|---|
| Policy types | `ghost-launcher/src/components/gatekeeper.rs` | dodać `SybilSoftSignals`, `SybilPolicyDiagnostics` lub analogiczne pola w decision layer |
| Verdicts | `ghost-launcher/src/components/gatekeeper.rs` | dodać `RejectSybilSoftExcess` oraz `RejectSybilInterference` |
| Policy engine | `ghost-launcher/src/components/gatekeeper_policy.rs` | policzyć sybil bucket wyłącznie z `feature_snapshot.sybil_resistance` |
| Decision logs | `ghost-launcher/src/components/gatekeeper.rs` + logger | eksportować `legacy_soft_points`, `sybil_soft_points`, patterny, lead signal |
| Config SSOT | `ghost-brain/src/config/ghost_brain_config.rs` | dodać nowe pola sybil-layer opisane niżej |
| Active rollout config | `ghost-brain/ghost_brain_config.toml` | jawnie dodać sybil-policy section zamiast polegać na kodowych neutral defaults |
| Tests | `ghost-launcher/tests/*`, policy unit tests | testy point-bucket, combo-veto, degraded semantics, replay drift |

---

## 9. Finalna powierzchnia configu

## 9.1. Pola istniejące, które należy wykorzystać

Już istnieją i powinny pozostać podstawą flagowania:

- `min_fee_topology_diversity_index`
- `max_dev_buyer_infrastructure_affinity`
- `min_spend_fraction_divergence`
- `min_demand_elasticity_score`
- `max_signer_cross_pool_velocity`
- `max_funding_source_concentration`
- `soft_penalty_low_ftdi`
- `soft_penalty_high_dbia`
- `soft_penalty_low_sfd`
- `soft_penalty_inelastic_demand`
- `soft_penalty_high_cpv`
- `soft_penalty_high_fsc`
- `soft_penalty_high_dbia_low_ftdi_combo`

## 9.2. Pola, które trzeba dodać, żeby temat był naprawdę domknięty

Bez tych pól plan pozostanie półśrodkiem:

- `max_sybil_soft_points: u8`
- `dev_unknown_max_sybil_soft_points: u8`
- `soft_penalty_low_des_low_sfd_combo: u8`
- `soft_penalty_high_cpv_low_des_combo: u8`
- `soft_penalty_high_fsc_high_cpv_combo: u8`
- `enable_sybil_interference_layer: bool`
- `enable_sybil_combo_veto: bool`

Opcjonalnie, ale sensownie:

- `emit_sybil_meta_score: bool`
- `require_ready_fsc_for_combo_veto: bool`

## 9.3. Zasada konfiguracyjna

Po merge'u kodu nowe pola nadal powinny startować neutralnie:

- wszystkie sybil penalties = `0`,
- `max_sybil_soft_points = 255`,
- `dev_unknown_max_sybil_soft_points = 255`,
- `enable_sybil_interference_layer = false`,
- `enable_sybil_combo_veto = false`.

Dopiero jawne ustawienie rollout configu ma przenieść system z telemetry-only do policy-active.

---

## 10. Rekomendowana kalibracja startowa

## 10.1. Progi metryk

Ten dokument **nie zamraża arbitralnych wartości liczbowych progów metryk** bez rozkładów rolloutowych.

Zamiast tego progi mają być ustawiane z paper-burnin / replay na zasadzie:

- dla metryk typu `low = suspicious` (`FTDI`, `SFD`, `DES`) używać dolnego percentyla organicznego baseline,
- dla metryk typu `high = suspicious` (`DBIA`, `CPV`, `FSC`) używać górnego percentyla organicznego baseline,
- każda wartość startowa musi być potwierdzona diffem na replay i ręcznym przeglądem false positives.

## 10.2. Startowe punkty sybil bucketu

To jest **punkt startowy do bake**, nie dogmat:

| Flaga / pattern | Startowe punkty |
|---|---:|
| `low_des` | `3` |
| `low_sfd` | `2` |
| `low_ftdi` | `1` |
| `high_dbia` | `1` |
| `high_cpv` | `1` |
| `high_fsc` | `1` |
| `high_dbia_low_ftdi_combo` | `2` |
| `low_des_low_sfd_combo` | `2` |
| `high_cpv_low_des_combo` | `1` |
| `high_fsc_high_cpv_combo` | `2` |

## 10.3. Startowy próg odcięcia

Bezpieczny punkt startowy po telemetry bake:

- `max_sybil_soft_points = 6`
- `dev_unknown_max_sybil_soft_points = 5`

Interpretacja:

- pojedynczy `low_des` jeszcze nie ubija,
- `low_des + low_sfd + combo` już może odcinać,
- `high_dbia + low_ftdi` bez dalszego potwierdzenia zwykle nie powinien jeszcze sam zabić,
- CPV/FSC wzmacniają case, ale nie dominują go na starcie.

---

## 11. Etapy wdrożenia końcowego

## 11.1. Etap A — policy plumbing bez efektu na verdict

Cel:

- dodać nowy bucket, nowe log fields, nowe verdict types,
- utrzymać **zero decision drift**.

Ustawienia:

- `enable_sybil_interference_layer = false` lub wszystkie sybil penalties = `0`,
- `max_sybil_soft_points = 255`,
- combo veto off.

Exit criteria:

- logi pokazują osobno legacy i sybil bucket,
- replay daje zero drift,
- degraded metrics nie penalizują.

## 11.2. Etap B — aktywacja lokalnych metryk

Aktywować:

- `DES`,
- `SFD`,
- `FTDI`,
- `DBIA`,
- `high_dbia_low_ftdi_combo`.

Nie aktywować jeszcze produkcyjnie:

- `FSC`,
- combo-veto,
- agresywnego `CPV`.

Exit criteria:

- sybil bucket wpływa na część verdictów,
- false positives są akceptowalne na replay,
- `high_dbia && high_ftdi` nie zachowuje się jak cabal proxy.

## 11.3. Etap C — ostrożne dołączenie CPV

Warunki:

- runtime metrics indeksu są stabilne,
- warmup działa deterministycznie,
- brak runaway memory.

Aktywacja:

- `soft_penalty_high_cpv`,
- ewentualnie `soft_penalty_high_cpv_low_des_combo`.

Exit criteria:

- CPV daje dodatkową wartość, a nie szum,
- cold index i degraded path nadal nie penalizują.

## 11.4. Etap D — aktywacja FSC

Warunki obowiązkowe:

- pełny authoritative funding stream,
- stabilna klasyfikacja neutral funderów,
- brak sztucznych clusterów od CEX hot wallets,
- potwierdzona gotowość runtime + replay.

Aktywacja:

- `soft_penalty_high_fsc`,
- ewentualnie `soft_penalty_high_fsc_high_cpv_combo`.

Exit criteria:

- `FSC` nie robi oczywistych false positives,
- `FSC_FUNDING_STREAM_UNAVAILABLE` oraz `FSC_INSUFFICIENT_KNOWN_SOURCES` nie nakładają kary,
- rollout telemetry potwierdza sensowne baseline distribution.

### 11.4.1. D0 — gate gotowości przed pierwszą realną karą FSC

Przed pierwszym włączeniem `soft_penalty_high_fsc` trzeba przejść jawny gate gotowości.

#### Checklista operacyjna

1. **Zakres Etapu C jest zamrożony.**
   - nie retune'ować już `CPV`,
   - nie zmieniać równolegle wag `DES/SFD/FTDI/DBIA`,
   - pierwszy drift po wejściu w D ma być możliwy do przypisania do `FSC`, a nie do ruchomego celu.

2. **FSC oceniamy na próbce jakościowej, nie na samym `tx_count`.**
   - dla bake i ręcznego przeglądu traktować jako próbkę kalibracyjną przede wszystkim poole z:
     - `buy_count >= 5`,
     - `unique_signers >= 4`.
   - poole poniżej tego progu pozostają telemetryczne i nie powinny prowadzić kalibracji `FSC`.

3. **`FSC_FUNDING_STREAM_UNAVAILABLE` nie może dominować próbki kalibracyjnej.**
   - ten reason może występować incydentalnie,
   - ale jeżeli staje się częstym stanem w jakościowych poolach, D nie jest gotowe do aktywacji kary.

4. **`FSC_INSUFFICIENT_KNOWN_SOURCES` nie może maskować większości sensownych przypadków.**
   - jeżeli duża część dobrych jakościowo pooli kończy z tym reasonem, najpierw trzeba poprawić coverage źródeł lub klasyfikację neutralnych funderów.

5. **Neutralni funderzy muszą być ręcznie sprawdzeni na próbce wysokiego `FSC`.**
   - przed aktywacją przejrzeć co najmniej top przypadki z najwyższym `funding_source_concentration`,
   - osobno obejrzeć przypadki przechodzące i przypadki graniczne / odcinane,
   - potwierdzić, że CEX hot wallets nie tworzą sztucznych clusterów.

6. **Degraded semantics musi pozostać nienaruszone.**
   - `None` i degraded `FSC` nadal muszą dawać `0` kary,
   - `FSC` nie może wejść do aktywnego patternu, jeśli jego gotowość jest niepełna.

#### Decyzja po D0

- jeśli wszystkie powyższe warunki są spełnione -> przejść do D1,
- jeśli nie -> zostać w telemetry-only dla `FSC` i nie aktywować kary.

### 11.4.2. D1 — konserwatywny start configu FSC

Pierwsze wejście D ma być celowo nudne. Nudne rollouty są niedoceniane, a szkoda.

#### Rekomendowana konfiguracja startowa

1. **Nie ruszać legacy bucketu.**
   - nie zmieniać `max_soft_points`,
   - nie zmieniać `dev_unknown_max_soft_points`,
   - nie robić równoległego retuningu starego scoringu.

2. **Utrzymać aktywny sybil bucket, ale bez combo-veto.**
   - `enable_sybil_interference_layer = true`,
   - `enable_sybil_combo_veto = false`.

3. **Włączyć wyłącznie lekką karę solo dla `high_fsc`.**
   - rekomendowany start: `soft_penalty_high_fsc = 1`.

4. **Nie aktywować jeszcze produkcyjnie `high_fsc_high_cpv_combo`.**
   - rekomendowany start: `soft_penalty_high_fsc_high_cpv_combo = 0`.

5. **Nie zmieniać w tym samym kroku progów innych metryk sybil.**
   - `DES/SFD/FTDI/DBIA/CPV` zostają takie jak na końcu Etapu C,
   - chodzi o izolację wpływu `FSC`.

6. **Jeżeli pole istnieje, utrzymać twardy guard gotowości dla combo-veto.**
   - `require_ready_fsc_for_combo_veto = true`.

#### Minimalna zasada wdrożeniowa

Pierwszy rollout Etapu D ma odpowiedzieć tylko na pytanie:

> czy lekki `high_fsc` poprawia decyzje jako corroborator, bez generowania głupich false positives?

Nie ma jeszcze odpowiadać na pytanie, czy `FSC` nadaje się do agresywnego combo-veto.

### 11.4.3. D2 — replay i bake po aktywacji FSC

Po włączeniu D trzeba wykonać osobny bake porównawczy względem końca Etapu C.

#### Checklista replay / bake

1. **Porównać drift Stage C -> Stage D.**
   - ile verdictów zmieniło się wyłącznie po wejściu `FSC`,
   - czy drift dotyczy głównie pooli już wcześniej granicznych,
   - czy drift nie wynika z równoległych zmian w innych bucketach.

2. **Ręcznie przejrzeć najwyższe przypadki `high_fsc`.**
   - osobno przypadki, które po D zostały odcięte,
   - osobno przypadki, które pozostały BUY mimo wysokiego `FSC`,
   - potwierdzić, że przypadki organiczne z neutralnym fundingiem nie są sztucznie zlepione.

3. **Sprawdzić relację `FSC` do `CPV`.**
   - `FSC` ma wnosić nową informację fundingową,
   - nie powinno być tylko duplikatem rotacji signerów z Etapu C.

4. **Potwierdzić brak kar na ścieżkach degraded.**
   - `FSC_FUNDING_STREAM_UNAVAILABLE` -> `0` kary,
   - `FSC_INSUFFICIENT_KNOWN_SOURCES` -> `0` kary,
   - `FSC_ROLLING_STATE_UNAVAILABLE` -> `0` kary.

5. **Sprawdzić explainability logów.**
   - buy log musi jasno pokazywać `sybil_soft_points`,
   - musi być widoczne, czy `FSC` podbiło tylko sybil bucket,
   - musi być widoczne, że combo-veto jest nadal wyłączone.

6. **Potwierdzić brak regresji runtime.**
   - brak memory runaway,
   - brak oczywistego wzrostu kosztu lookupów,
   - brak zatoru związanego z ingestem funding streamu.

### 11.4.4. D3 — reguły go / no-go po pierwszym bake

#### GO do pełnego Etapu D

Można uznać D za udane i zostawić `soft_penalty_high_fsc` aktywne, jeśli łącznie:

1. drift jest explainable i dotyczy głównie przypadków już wcześniej granicznych lub podejrzanych,
2. `high_fsc` solo nie generuje serii oczywistych false positives,
3. neutralni funderzy nie tworzą sztucznej koncentracji,
4. degraded ścieżki pozostają bezkarne,
5. runtime i funding stream są stabilne.

#### NO-GO dla dalszej promocji FSC

Nie przechodzić dalej do mocniejszych patternów ani do Etapu E, jeśli występuje którykolwiek z poniższych stanów:

1. `FSC_FUNDING_STREAM_UNAVAILABLE` często pojawia się w próbce kalibracyjnej,
2. `FSC_INSUFFICIENT_KNOWN_SOURCES` dominuje w jakościowych poolach,
3. pierwszy istotny drift pochodzi głównie z solo `high_fsc`, a nie z corroboracji istniejącego risku,
4. organiczne poole finansowane z CEX hot wallets są sztucznie windowane przez `FSC`,
5. pojawia się kara mimo degraded / `None` state.

W takim przypadku D należy cofnąć do telemetry-only lub zostawić tylko obserwację bez aktywnej kary i wrócić do higieny funding streamu / neutralnych funderów.

## 11.5. Etap E — whitelistowany combo-veto

Dopiero po wszystkich wcześniejszych bake:

- `enable_sybil_combo_veto = true`

Na start dopuścić wyłącznie bardzo wąskie patterny:

1. `high_dbia && low_ftdi && low_sfd`
2. `low_des && low_sfd && (high_dbia || low_ftdi)`
3. `high_fsc && high_cpv && (low_des || low_sfd)`

I tylko jeśli wszystkie użyte metryki są gotowe, nie-degraded i pochodzą z kanonicznego snapshotu.

---

## 12. Logging i explainability

Finalny buy log / decision log powinien osobno eksponować:

- `legacy_soft_points`
- `legacy_soft_threshold`
- `sybil_soft_points`
- `sybil_soft_threshold`
- `legacy_soft_flags`
- `sybil_soft_flags`
- `sybil_lead_signal`
- `sybil_interference_patterns`
- `sybil_meta_score` (jeśli emitowany)
- `sybil_metric_degraded_reasons`

Powód jest prosty: operator ma widzieć, **czy pool przeszedł dlatego, że sybil bucket był czysty, czy dlatego, że sybil bucket był neutralny / nieaktywny**.

---

## 13. Testy wymagane, żeby temat uznać za zamknięty

## 13.1. Policy unit tests

1. `high_dbia && high_ftdi` nie dostaje takiej samej kary jak `high_dbia && low_ftdi`.
2. `low_des` ma największą wagę pojedynczą.
3. `low_des + low_sfd + combo` może przebić `max_sybil_soft_points`.
4. `high_cpv` solo nie daje agresywnego rejectu.
5. `high_fsc` solo nie daje veto.
6. degraded / `None` metryki dają `0` kary.
7. `max_soft_points = 255` nie blokuje działania `max_sybil_soft_points`.

## 13.2. Replay tests

1. neutralny sybil config -> zero drift,
2. Stage B config -> przewidywalny drift tylko dla sybil-case’ów,
3. CPV/FSC aktywne -> brak driftu z powodu cold/warmup artifactów,
4. combo-veto -> drift ograniczony do jawnie whitelistowanych patternów.

## 13.3. Runtime / integration tests

1. CPV ready vs not-ready,
2. FSC stream available vs unavailable,
3. neutral funders nie budują sztucznej koncentracji,
4. buy log zawiera wszystkie nowe pola explainability.

---

## 14. Definition of Done

Temat 6 metryk jest zamknięty dopiero wtedy, gdy łącznie spełnione są wszystkie warunki:

1. policy konsumuje metryki wyłącznie z `MaterializedFeatureSet.sybil_resistance`,
2. legacy i sybil buckets są rozdzielone semantycznie i telemetrycznie,
3. sybil-policy może działać niezależnie od legacy `max_soft_points`,
4. degraded / `None` metrics nigdy nie penalizują,
5. `DES` prowadzi, `DBIA+FTDI` jest najmocniejszym patternem strukturalnym, `SFD` jest corroboratorem, `CPV/FSC` są wzmacniaczami po warmupie,
6. aktywny rollout config jawnie definiuje sybil-policy, zamiast polegać na ukrytych defaultach,
7. replay z neutralnym configiem daje zero drift,
8. replay z aktywnym sybil configiem daje oczekiwany, explainable drift,
9. combo-veto działa tylko dla whitelistowanych patternów i tylko po bake,
10. buy log mówi jasno **dlaczego** verdict przeszedł lub został odcięty przez sybil layer.

---

## 15. Końcowa rekomendacja

Jeżeli celem jest plan **kompletny, kompatybilny z resztą metryk i sensowny produkcyjnie**, to finalny kierunek jest jeden:

**nie dodawać 6 metryk do legacy phase gates ani do zamrożonego legacy soft-score, tylko wdrożyć osobny `Sybil Interference` bucket z własnym progiem, własnym explainability, mocnymi combo-patternami i bardzo późnym, whitelistowanym combo-veto.**

To jest najkrótsza droga do systemu, który:

- naprawdę zaczyna filtrować sybilowe case’y,
- nie psuje starego Gatekeepera,
- nie myli telemetry z policy,
- i nie robi z `FSC` ani `CPV` pseudo-autorytarnych wyroczni zanim runtime i dane dojrzeją.