# MATRYCA SYBIL POLICY — PO FAZIE 5 vs STAN FINALNY — 2026-04-14

## 0. Cel dokumentu

Ten dokument rozdziela dwa różne pytania, które łatwo pomylić:

1. **co ma sens wdrażać operacyjnie już teraz, po ukończeniu Fazy 5**, gdy `FTDI/DBIA/SFD/DES/CPV` są w zasięgu, ale `FSC` nie jest jeszcze pełnym sygnałem produkcyjnym,
2. **jak ma wyglądać docelowa logika policy po ukończeniu całego planu**, czyli po domknięciu również `FSC`, funding streamu i pełnej warstwy interferencyjnej.

Dokument zakłada kontrakty z `PLAN_IMPLEMENTACJI_NOWYCH_METRYK.md` i ADR-0092 / ADR-0095:

- SSOT decyzji pozostaje `MaterializedFeatureSet.sybil_resistance`,
- `early_fingerprint` i `FINGERPRINT` pozostają observability, nie źródłem prawdy,
- `None` / degraded metrics nigdy nie mogą działać jak ukryta kara,
- w v1 nowe metryki nie powinny wejść jako samotne hard-fail kill switche,
- logika sybil ma być osobną warstwą policy, a nie mutacją istniejących phase gates.

---

## 1. Zasada główna

## Nie dodawać tych 6 metryk jako zwykłych legacy-phase thresholds

To oznacza:

- **nie** traktować ich jak kolejnych odpowiedników `min_avg_interval_ms`, `max_avg_interval_ms`, `max_hhi`,
- **nie** doklejać ich do istniejących faz 2/3/4/5/6,
- **nie** robić z pojedynczej wartości typu `DES < X` natychmiastowego hard-faila w pierwszym rolloutcie.

### Dlaczego

Bo nowe 6 metryk ma inną semantykę niż stare phase-gates:

- część z nich jest **lokalna** (`FTDI`, `DBIA`, `SFD`, `DES`),
- część jest **globalna / rolling-state dependent** (`CPV`, `FSC`),
- część ma sens głównie w **kombinacjach**, a nie solo,
- część wymaga **warmupu** i gotowości indeksu,
- część ma wysoką wartość dopiero po telemetry bake.

Wepchnięcie ich do legacy faz zrobiłoby semantyczny bałagan i podniosło ryzyko false positives.

---

## 2. Model docelowy — wspólny dla obu etapów

Docelowy kształt policy powinien być czterowarstwowy:

1. **Hard Fails** — bez zmian, dalej dla jawnych kill-switchy,
2. **Core Pass** — bez zmian, dalej dla viability / safety,
3. **Legacy Soft Signals** — bez zmian,
4. **Sybil Interference Layer** — nowa, osobna warstwa.

### Sybil Interference Layer powinna dawać

- `SybilSoftSignals`
- `sybil_soft_points`
- `sybil_interference_pattern`
- opcjonalnie `sybil_meta_score` (telemetry-first)

Finalnie:

- `legacy_soft_points`
- `sybil_soft_points`
- `total_soft_points = legacy_soft_points + sybil_soft_points`

---

## 3. Matryca operacyjna — co robić już teraz po Fazie 5

## 3.1. Co realnie zakładamy po Fazie 5

Po Fazie 5 zakładamy, że praktycznie gotowe do użycia policy są:

- `FTDI`
- `DBIA`
- `SFD`
- `DES`
- `CPV`

Natomiast:

- `FSC` **nie powinno jeszcze dostać realnej wagi produkcyjnej**, dopóki nie istnieje pełny funding stream i zweryfikowana lista neutralnych funderów.

## 3.2. Role metryk po Fazie 5

| Metryka | Stan po Fazie 5 | Rola policy teraz | Czy może działać solo? | Rekomendacja |
|---|---|---|---|---|
| `DES` | gotowa lokalnie | **Lead Signal** | tak, ale tylko jako silny soft signal | najwyższa waga w sybil bucketcie |
| `DBIA` | gotowa lokalnie | Structural signal | **nie** jako samodzielny wyrok | solo słaba/umiarkowana kara |
| `FTDI` | gotowa lokalnie | Structural counter-signal / corroborator | tak, ale słabsza niż DES | ważna głównie z DBIA |
| `SFD` | gotowa lokalnie | Capital-behavior corroborator | tak | średnio-wysoka kara |
| `CPV` | gotowy bounded index | Rotation corroborator | raczej nie | niska/średnia kara, tylko gdy index ready |
| `FSC` | jeszcze niepełny finalnie | telemetry placeholder | nie | **0 real penalty** |

## 3.3. Priorytet metryk po Fazie 5

### Poziom A — sygnał prowadzący

1. `DES`

### Poziom B — najmocniejsza kombinacja strukturalna

2. `DBIA + low FTDI`

### Poziom C — potwierdzenie kapitałowo-zachowaniowe

3. `SFD`
4. `CPV`

### Poziom D — jeszcze nieprodukcyjne po Fazie 5

5. `FSC`

## 3.4. Rekomendowane wzorce interferencji po Fazie 5

| Wzorzec | Interpretacja | Akcja po Fazie 5 |
|---|---|---|
| `low_des` | sygnał procesu trudny do spoofingu | wysoka kara soft, ale nie hard fail |
| `high_dbia && low_ftdi` | najmocniejsza strukturalna sygnatura cabal | bardzo wysoka kara soft |
| `low_des && low_sfd` | skoordynowany timing + podobne obciążenie kapitału | bardzo wysoka kara soft |
| `high_dbia && low_ftdi && low_sfd` | silna zgodność infrastruktury i wydatku | kandydat do przyszłego veto, teraz mocny soft |
| `high_cpv && low_des` | rotacja signerów zgodna z nienaturalnym procesem | średnio-wysoka kara soft |
| `high_dbia && high_ftdi` | możliwy shared retail bot, nie twardy cabal | telemetry / lekka kara albo brak |
| `high_cpv` solo | sama rotacja nie dowodzi cabal | niska kara lub tylko log |

## 3.5. Czego NIE robić po Fazie 5

1. **Nie robić `DES < X => hard reject`.**
2. **Nie robić `DBIA > X => reject`.**
3. **Nie używać `CPV` bez readiness / warmup guard.**
4. **Nie dawać `FSC` realnej kary produkcyjnej.**
5. **Nie kończyć obliczeń feature snapshotu wcześniej tylko dlatego, że DES jest niski.**
   - pełny snapshot jest potrzebny dla replay, explainability i telemetryki.

## 3.6. Rekomendowane punkty soft po Fazie 5 (punkt startowy, nie dogmat)

To jest **propozycja startowa do rollout bake**, nie finalna kalibracja.

| Flaga / pattern | Proponowane punkty |
|---|---:|
| `low_des` | `5` |
| `low_sfd` | `3` |
| `low_ftdi` | `2` |
| `high_dbia` | `1` |
| `high_cpv` | `1` |
| `high_dbia_low_ftdi_combo` | `4` |
| `high_cpv_low_des_combo` | `2` |
| `low_des_low_sfd_combo` | `3` |

### Uwaga

Jeżeli system nadal ma `max_soft_points = 255`, to sybil-soft nie będzie niczego realnie odcinał.
Wtedy faza po wdrożeniu tej logiki będzie nadal **telemetry-first**, co samo w sobie jest OK.

---

## 4. Matryca docelowa — po pełnym ukończeniu planu

## 4.1. Co dochodzi po pełnym planie

Po ukończeniu całego planu dochodzą brakujące właściwości:

- pełny `FSC`,
- funding transfer stream,
- neutral funder classification,
- pełna bounded-state observability dla FSC,
- dłuższy telemetry bake dla funding semantics,
- możliwość promowania wybranych patternów do mocniejszego verdictu.

## 4.2. Role metryk w stanie finalnym

| Metryka | Rola finalna | Czy solo może wpływać na decyzję? | Docelowa siła |
|---|---|---|---|
| `DES` | **Lead Signal** | tak, jako bardzo silny soft signal | najwyższa |
| `DBIA` | structural affinity | tylko słabo solo | średnia solo, wysoka w combo |
| `FTDI` | structural diversity / counter-signal | tak | średnia |
| `SFD` | spend-pattern corroborator | tak | średnio-wysoka |
| `CPV` | rotation corroborator | tak, ale ostrożnie | średnia |
| `FSC` | funding-source corroborator | tak, ale po bake | średnia-wysoka |

## 4.3. Priorytet finalny

### Poziom 1 — Lead Signals

1. `DES`
2. `high_dbia_low_ftdi_combo`

### Poziom 2 — Structural / capital amplifiers

3. `SFD`
4. `FSC`
5. `CPV`

### Poziom 3 — Meta / summary

6. `sybil_meta_score` (telemetry-first, ewentualnie później policy-backed)

## 4.4. Finalna tabela interferencji

| Konfiguracja | DES | DBIA | FTDI | SFD | FSC | CPV | Finalna interpretacja |
|---|---|---|---|---|---|---|---|
| Cabal industrialny | Low | High | Low | Low | Med/High | Med | najmocniejszy pattern cabal |
| Sybil network funded | Med/Low | Med | Med/Low | Med | High | High | funding + rotation cluster |
| Shared retail bot | High | High | High | Med | Low | Med | nie blokować agresywnie |
| Organic degen | High | Low | Med/High | High | Low | Low | pass / niski sybil risk |
| Cabal z multi-hop | Low | Med | Low | Low | Low/Med | Med | DES + SFD nadal prowadzą |
| Aggressive sniper swarm | Med | Low | High | High | Low | High | CPV podnosi risk, ale DES i FTDI bronią |

## 4.5. Co może awansować do mocniejszego verdictu finalnie

Dopiero po bake i replay diffach można rozważyć osobny werdykt typu:

- `RejectSybilInterference`

Ale tylko dla **kombinacji**, nie dla pojedynczych metryk.

### Kandydaci do promocji

1. `high_dbia && low_ftdi && low_sfd`
2. `low_des && low_sfd && (high_dbia || low_ftdi)`
3. `high_fsc && high_cpv && (low_des || low_sfd)`

### Niekandydaci do promocji solo

- `high_dbia` solo
- `high_cpv` solo
- `high_fsc` solo

---

## 5. Meta-score — jak traktować go rozsądnie

## 5.1. Ocena pomysłu

Pomysł z `meta_score` jest dobry jako:

- warstwa gray-zone,
- ranking offline,
- sygnał dla rollout bake,
- wejście do future MetaScorera.

## 5.2. Czego nie robić od razu

Nie używać `meta_score` jako jedynego production gate od pierwszego dnia.

### Dlaczego

Obecna powierzchnia configu jest lepsza do:

- boolean flags,
- penalties,
- combo patterns,

niż do pełnej normalizacji ciągłej dla wszystkich 6 metryk.

## 5.3. Rekomendowany status finalny

- `meta_score` liczyć i logować,
- używać do klasyfikacji `Organic / Gray / Strong Cabal`,
- dopiero później ewentualnie spiąć go z policy jako dodatkowy booster, nie jedyny sędzia.

---

## 6. Jak to osadzić w configu

## 6.1. Wniosek dla aktywnego `ghost_brain_config.toml`

W aktywnym TOML **nie ma jeszcze pól sybil-policy** mimo że kod konfiguracyjny już je wspiera.

To oznacza, że po wdrożeniu policy-layer warto dodać jawnie do `[gatekeeper_v2]` sekcję podobną do tej:

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

## 6.2. Stan po Fazie 5 — jak konfigurować

Po Fazie 5:

- `FSC` zostawić neutralnie,
- `CPV` ustawić ostrożnie,
- najwięcej uwagi dać `DES`, `SFD`, `DBIA+FTDI`.

## 6.3. Stan finalny — jak konfigurować

Po pełnym planie:

- aktywować wszystkie pola,
- dodać jawne pattern-based logging,
- rozważyć osobny próg dla `RejectSybilInterference`.

---

## 7. Rekomendacja końcowa

## Po Fazie 5

**Wdrażać już teraz jako osobny sybil-soft bucket z DES jako leadem, `DBIA+low FTDI` jako najmocniejszą kombinacją strukturalną, `SFD` jako głównym corroboratorem i `CPV` jako sygnałem pomocniczym. FSC jeszcze nie traktować produkcyjnie.**

## Po pełnym ukończeniu planu

**Zachować model interferencyjny, dołączyć FSC jako pełnoprawny sygnał kapitałowy i dopiero wtedy rozważyć promowanie wybranych kombinacji do dedykowanego verdictu typu `RejectSybilInterference`.**

## Jednozdaniowe podsumowanie

- **Po Fazie 5:** sybil-soft + interference, bez hard veto.
- **Po finale:** sybil-soft + interference + wybrane combo-veto po telemetry bake.
