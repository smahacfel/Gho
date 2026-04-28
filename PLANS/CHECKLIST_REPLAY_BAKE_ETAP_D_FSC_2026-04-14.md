# CHECKLISTA REPLAY / BAKE — ETAP D (FSC) — 2026-04-14

## 0. Cel

Ta checklista służy do **odklikania na żywo** wejścia w Etap D, czyli pierwszej konserwatywnej aktywacji `FSC` w policy.

Zakładany startowy config D1:

- `enable_sybil_interference_layer = true`
- `enable_sybil_combo_veto = false`
- `max_funding_source_concentration = 0.60`
- `soft_penalty_high_fsc = 1`
- `soft_penalty_high_fsc_high_cpv_combo = 0`
- brak zmian w legacy `max_soft_points`
- brak zmian w innych progach / karach sybil względem końca Etapu C

---

## 1. D0 — gate gotowości przed aktywacją

### 1.1. Freeze zakresu

- [ ] Etap C jest zamrożony — brak równoległego retuningu `CPV`
- [ ] Brak równoległych zmian wag `DES/SFD/FTDI/DBIA`
- [ ] Brak zmian w legacy `max_soft_points`
- [ ] Brak zmian w `dev_unknown_max_soft_points`

### 1.2. Jakość danych FSC

- [ ] Funding stream jest aktywny i stabilny
- [ ] `FSC_FUNDING_STREAM_UNAVAILABLE` nie dominuje jakościowych pooli
- [ ] `FSC_INSUFFICIENT_KNOWN_SOURCES` nie dominuje jakościowych pooli
- [ ] `FSC_ROLLING_STATE_UNAVAILABLE` nie pojawia się systemowo
- [ ] Neutralni funderzy są zaktualizowani / sprawdzeni

### 1.3. Próbka kalibracyjna

- [ ] Do bake używamy przede wszystkim pooli z `buy_count >= 5`
- [ ] Do bake używamy przede wszystkim pooli z `unique_signers >= 4`
- [ ] Poole poniżej tego progu nie prowadzą kalibracji progów `FSC`

### 1.4. Manualny sanity check

- [ ] Przejrzane top przypadki z najwyższym `funding_source_concentration`
- [ ] Osobno przejrzane przypadki przechodzące (BUY)
- [ ] Osobno przejrzane przypadki graniczne / odcinane
- [ ] Brak oczywistego sztucznego clusteringu przez CEX hot wallets

### Decyzja D0

- [ ] GO do D1
- [ ] NO-GO — zostać w telemetry-only

Notatka:

- Decyzja / powód: ..............................................................
- Data / operator: ..............................................................

---

## 2. D1 — patch configu aktywacyjnego

### 2.1. Pola, które mają być ustawione

- [ ] `max_funding_source_concentration = 0.60`
- [ ] `soft_penalty_high_fsc = 1`
- [ ] `soft_penalty_high_fsc_high_cpv_combo = 0`
- [ ] `enable_sybil_interference_layer = true`
- [ ] `enable_sybil_combo_veto = false`
- [ ] `require_ready_fsc_for_combo_veto = true`

### 2.2. Pola, które mają pozostać bez zmian

- [ ] `max_soft_points = 255`
- [ ] `dev_unknown_max_soft_points = 255`
- [ ] `soft_penalty_high_cpv = 1`
- [ ] `soft_penalty_high_cpv_low_des_combo = 0`
- [ ] `soft_penalty_low_ftdi = 1`
- [ ] `soft_penalty_high_dbia = 1`
- [ ] `soft_penalty_low_sfd = 2`
- [ ] `soft_penalty_inelastic_demand = 3`
- [ ] `soft_penalty_high_dbia_low_ftdi_combo = 2`
- [ ] `soft_penalty_low_des_low_sfd_combo = 2`
- [ ] `max_sybil_soft_points = 6`
- [ ] `dev_unknown_max_sybil_soft_points = 5`

### 2.3. Zasada wdrożeniowa

- [ ] Aktywujemy tylko lekki solo `high_fsc`
- [ ] Nie aktywujemy jeszcze `high_fsc_high_cpv_combo`
- [ ] Nie aktywujemy combo-veto

---

## 3. D2 — replay / bake po aktywacji

### 3.1. Drift Stage C -> Stage D

- [ ] Policzone, ile verdictów zmieniło się po samym wejściu `FSC`
- [ ] Drift opisany jako procent / liczba przypadków
- [ ] Zweryfikowane, że drift nie wynika z innych równoległych zmian
- [ ] Zweryfikowane, że drift dotyczy głównie pooli granicznych lub podejrzanych

Wynik:

- Liczba zmienionych verdictów: ................................................
- % driftu: ....................................................................

### 3.2. Highest `high_fsc` review

- [ ] Przejrzane top odcięte przypadki z `high_fsc`
- [ ] Przejrzane top BUY przypadki z `high_fsc`
- [ ] Sprawdzone, czy `FSC` wnosi nową informację względem `CPV`
- [ ] Sprawdzone, czy `high_fsc` solo nie tworzy serii oczywistych false positives

Notatki:

- FP przykłady: ................................................................
- TP przykłady: ................................................................

### 3.3. Degraded semantics

- [ ] `FSC_FUNDING_STREAM_UNAVAILABLE` daje `0` kary
- [ ] `FSC_INSUFFICIENT_KNOWN_SOURCES` daje `0` kary
- [ ] `FSC_ROLLING_STATE_UNAVAILABLE` daje `0` kary
- [ ] `None` nie generuje `high_fsc`

### 3.4. Explainability

- [ ] Buy log pokazuje `sybil_soft_points`
- [ ] Buy log pokazuje `sybil_soft_flags`
- [ ] Buy log pokazuje `sybil_metric_degraded_reasons`
- [ ] Widać, że combo-veto jest nadal wyłączone
- [ ] Widać, czy `FSC` tylko podbiło sybil bucket, czy nic nie zmieniło

### 3.5. Runtime health

- [ ] Brak memory runaway
- [ ] Brak oczywistego wzrostu kosztu lookupów
- [ ] Brak zatoru ingestu funding streamu
- [ ] Brak lawiny błędów / reconnect storms wpływających na coverage `FSC`

---

## 4. D3 — decyzja GO / NO-GO

### GO — można zostawić D aktywne

- [ ] Drift jest explainable
- [ ] `high_fsc` solo nie generuje serii głupich false positives
- [ ] Neutralni funderzy nie tworzą sztucznej koncentracji
- [ ] Degraded ścieżki pozostają bezkarne
- [ ] Runtime i funding stream są stabilne

### NO-GO — wracamy do telemetry-only lub zostawiamy obserwację bez kary

- [ ] `FSC_FUNDING_STREAM_UNAVAILABLE` często dominuje próbkę kalibracyjną
- [ ] `FSC_INSUFFICIENT_KNOWN_SOURCES` dominuje jakościowe poole
- [ ] Istotny drift pochodzi głównie z solo `high_fsc`
- [ ] Organiczne poole z CEX fundingiem są sztucznie windowane
- [ ] Pojawia się kara mimo degraded / `None`

### Finalna decyzja

- [ ] GO — zostawić D1 aktywne
- [ ] ROLLBACK — wrócić do `soft_penalty_high_fsc = 0`
- [ ] HOLD — zostać na telemetry / zebrać więcej danych

Podpis / data / operator:

- ..............................................................................

---

## 5. Notatki końcowe

- Etap D nie jest jeszcze zgodą na Etap E.
- Samo pozytywne przejście D1 nie oznacza zgody na `enable_sybil_combo_veto = true`.
- Do Etapu E przechodzimy dopiero po osobnym bake i ręcznej walidacji patternów combo z udziałem `FSC`.