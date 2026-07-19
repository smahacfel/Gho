# ADR-8D: HET-PM V2 — v1a terminal admission release reconciliation

Status: `IMPLEMENTED CORRECTIVE EVIDENCE FIX / COMPLETED v1a IS DIAGNOSTIC ONLY`

Typ: ADR-8D / shadow post-buy / promotion evidence / lifecycle reconciliation

Data: `2026-07-19`

Repo: `smahacfel/Gho`

Branch: `agent/het-pm-v2-reproducible-validation`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`, §19.9a oraz
promotion-evidence prerequisite przed odrębnym PR B.

Poziom ryzyka: `LOW` dla authority i capacity, `MEDIUM` dla ważności
prospective evidence. Zmiana nie dotyka HET/V1/TimeStop policy, progów,
quote'ów, proposal/apply, canonical terminal commit, rzeczywistego zwalniania
capacity ani shadow/live mode.

## D0. Decyzja

Zakończony native 30-minute run
`validation-v1a-30m` nie jest dopuszczony jako finalny prospective validation
run. Jest użyteczny diagnostycznie, ale ujawnił brak durable admission evidence
dla jednego poprawnie zwolnionego slotu.

Przed kolejnym prospective runem runtime musi zawsze wyemitować dokładnie
jeden typed `terminal_release` admission record po terminal notification:

- `release_status = released`, gdy terminal watcher sam zwolnił slot;
- `release_status = already_released`, gdy lifecycle zwolnił go wcześniej.

Obie wartości oznaczają poprawne zakończenie reservation lifecycle. Różnią
jedynie moment i owner wcześniejszej operacji, nie fakt zwolnienia capacity.

## D1. Fakty z validation-v1a-30m

Run wystartował `2026-07-19T08:34:05Z`, otrzymał native `SIGINT` dokładnie po
`1800` sekundach i zakończył graceful shutdown o `2026-07-19T09:04:19Z`.
HET writer health i admission writer health były kompletne, bez queue drops,
write failures lub timeoutów terminalnych.

Promotion evaluation wykazał jednak `admission_missing_release_count > 0`.
Dokładna korelacja wskazała pojedynczy candidate, dla którego admission JSONL
zawierał `post_buy_submitted`, `handoff_accepted` i
`monitoring_registered`, lecz nie zawierał `terminal_release`.

Runtime log dla tej samej identity zawierał warning:

```text
shadow position slot already released before terminal notification
```

Oznacza to, że capacity nie wyciekło. Brakował wyłącznie rekord evidence
potrzebny do dwukierunkowej reconciliation Gate 1.

## D2. Przyczyna źródłowa

`spawn_shadow_terminal_watcher()` uprzednio traktował wynik
`PositionLimitTracker::release(slot_id) == false` jako warning-only branch.
W tej gałęzi nie konstruował i nie enqueue'ował `post_buy_admission_v1`
z `stage = terminal_release`.

To jest niepoprawne dla kontraktu evidence: `false` znaczy tylko, że slot był
już zwolniony przez prawidłową, wcześniejszą ścieżkę lifecycle. Nie znaczy, że
terminal notification nie posiada typed release outcome.

## D3. Korekta runtime i analyzera

Terminal watcher teraz zawsze tworzy pre-serializowany admission record po
odebraniu terminal disposition. Najpierw wykonuje istniejące pojedyncze
`release(slot_id)`, następnie dobiera jeden z dwóch statusów opisanych w D0 i
przekazuje rekord do już istniejącego bounded, non-blocking admission writer.

Nie dodano filesystem I/O do event path, dodatkowego release, retry ani
nowego ownera. Warning dla `already_released` pozostaje diagnostyczny.

Promotion analyzer przestaje traktować każde administracyjne cenzorowanie
na końcu kontrolowanego horyzontu jako causal lifecycle violation. Taki censor
pozostaje licznikiem i elementem terminal-or-censor coverage. Wyłącznie
`candidate_bearing_censored_count` jest causal failure, zgodnie z wcześniej
zamrożoną zasadą: position z promotable candidate nie może zniknąć bez
terminal economic follow-up.

## D4. Testy i inwarianty

Dodano test `shadow_terminal_watcher_writes_admission_already_released_terminal_release`,
który:

1. rejestruje slot;
2. zwalnia go przed notification;
3. dostarcza terminal `SimulationBlocked`;
4. wymaga durable `terminal_release` z
   `release_status = already_released` i typed reason;
5. potwierdza zero aktywnych slotów.

Test analyzera potwierdza, że non-candidate controlled-horizon censor jest
widoczny w `censored_position_count`, ale nie staje się candidate-bearing
economic/causal failure. Istniejący test nadal wymaga FAIL dla candidate-bearing
censoringu.

Zachowane inwarianty:

- V1 pozostaje jedynym proposal/apply/terminal/capacity ownerem;
- HET-PM V2 pozostaje observe-only;
- terminal watcher nie zmienia wyniku release, a tylko go dokumentuje;
- admission writer nadal używa bounded `try_send`, więc evidence nie blokuje
  registration, lifecycle ani capacity;
- historical JSONL nie jest przepisywany.

## D5. Konsekwencja dla kolejnych runów

Ta korekta zmienia release binary oraz promotion tool. Istniejący locked
criteria/provenance contract nie może być użyty do nowej prospective pary.
Przed kolejnym `validation-v1a` należy:

1. zbudować clean release binary z tej korekty;
2. ponownie wykonać canonical provenance lock dla obu exact run-configów;
3. sprawdzić testy runtime i promotion toola;
4. uruchomić nowy 30-minute `validation-v1a-30m-r2` z nowym launch cohort;
5. dopiero po jego zamknięciu ocenić, czy można uruchomić niezależny
   `validation-v1b`.

Nie wolno relabelować ani edytować artefaktów zakończonego v1a. Nie wolno też
interpretować tej korekty jako zgody na authority cutover: PR B runtime nadal
jest osobnym, przyszłym zakresem i wymaga poprawnej, source-recomputed
promotion evidence.

## D6. Rollback

Rollback polega na zatrzymaniu kolejnego runu przed startem i pozostawieniu go
bez manifestu promotion. Nie należy usuwać ani modyfikować historycznych
artefaktów. Powrót do warning-only branch byłby regresją evidence contractu i
wymagałby jawnej, nowej decyzji właściciela projektu.
