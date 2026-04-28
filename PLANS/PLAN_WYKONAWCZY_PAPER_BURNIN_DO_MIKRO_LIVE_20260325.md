# Wykonawczy plan przejścia od stabilnego shadow run do paper burn-in i pierwszego mikro-live

## Cel tego dokumentu

Ten dokument nie ma opisywać "co byłoby fajnie zrobić". To jest **wykonawczy plan realizacji** kolejnego etapu Ghosta po ustabilizowaniu runtime shadow. Celem nie jest już samo udowodnienie, że hot path działa szybko. Celem jest metodyczne przejście od:

- stabilnego `shadow_only`,
- przez pełny `paper + shadow_only`,
- do pierwszego kontrolowanego `dual + live_and_shadow`

przy zachowaniu minimalnego blast radius, jawnych gate'ów oraz uczciwej odpowiedzi na pytanie:

> czy system po pełnym lifecycle pozycji i po realnych kosztach execution nadaje się do mikro-live, czy nadal wymaga tylko dalszego burn-in / researchu?

---

## Wniosek wprost

Na dziś repo nie powinno przechodzić od razu do mikro-live. Shadow udowodnił już wiele, ale nie wszystko.

Udowodnione zostało przede wszystkim:

- szybkie event detection i following,
- sprawny shadow buy path,
- sensowna architektura ingest -> decision -> execution,
- działający trzon recovery / durability,
- gotowość do porównawczego execution path.

Nie zostało jeszcze domknięte:

1. twarde correctness / baseline build-test gate,
2. jednoznaczna semantyka execution configu,
3. egzekucja bulkhead safety w kanonicznej ścieżce BUY,
4. jawna i nieumowna aktywacja WAL,
5. pełna operationalizacja paper position lifecycle jako etapu obowiązkowego przed live,
6. runbook produkcyjny i preflight,
7. spójny system liczenia net PnL i operational loss dla rollout decision.

W praktyce oznacza to, że następny program prac powinien być podzielony na **7 większych PR-ów**, wykonywanych dokładnie w podanej kolejności.

---

## Założenia wykonawcze

### 1. Kolejność PR-ów jest obowiązkowa

Najpierw correctness i safety, dopiero potem operational hardening, dopiero potem paper burn-in, dopiero na końcu dual-mode mikro-live.

Nie wolno:

- uruchamiać mikro-live przed zakończeniem PR-5,
- skalować ekspozycji w PR-6,
- mieszać cleanupu sekretów z rolloutem live,
- próbować "nadgonić" braków correctness samą telemetrią.

### 2. Shadow nie jest już głównym celem

Shadow ma pozostać aktywny, ale od tego momentu pełni rolę:

- warstwy porównawczej,
- źródła divergence diagnostics,
- ubezpieczenia poznawczego przy paper i live.

To oznacza, że nie planujemy już programu prac pod hasłem "jeszcze dłuższy shadow", tylko pod hasłem:

> "domknąć correctness, uruchomić paper lifecycle, porównać shadow/paper/live i dopiero wtedy dopuścić realny mikrokapitał".

### 3. Każdy PR musi mieć twardy merge gate

Każdy PR ma mieć:

- jasno określony cel semantyczny,
- listę plików obowiązkowych,
- obowiązkowy zestaw testów / walidacji,
- wyraźnie wskazane rzeczy poza zakresem,
- warunek merge.

### 4. Dopóki nie zostanie spełniony gate, rollout nie przechodzi dalej

Nie wolno przejść do kolejnego PR-u rolloutowego tylko dlatego, że "większość działa". Każdy etap ma zamykać realny kontrakt runtime.

---

## Stan wejściowy, na którym opiera się ten plan

### 1. Shadow path jest realnie użyteczny

`logs/shadow_run/buys.jsonl` pokazuje kilka tysięcy prób shadow buy oraz niskie opóźnienia rzędu ~100 ms dla przygotowania i zakończenia symulacji. To oznacza, że detection, bus i sam shadow submit/simulate nie są już eksperymentem.

### 2. Repo ma gotowe rozróżnienie execution modes

W kodzie istnieją:

- `execution.execution_mode = live | paper | dual`
- `trigger.entry_mode = live | dry_run_mock | shadow_only | live_and_shadow`

To jest dobra baza, ale na dziś config i runtime nie są jeszcze semantycznie domknięte.

### 3. Paper lifecycle już istnieje, ale nie jest jeszcze rolloutowym SSOT dla etapu przed-live

`ghost-brain/src/execution/paper_lifecycle.rs` oraz `ghost-launcher/src/components/post_buy_runtime.rs` dają już:

- entry,
- fill polling,
- AEM tick loop,
- exit,
- close events.

To jest bardzo ważne: repo ma już komponent, na którym można oprzeć pełny etap paper burn-in, zamiast dalej ograniczać się do shadow buy telemetry.

### 4. Największe luki są dziś w correctness i operational contracts

Najpoważniejsze obszary do domknięcia:

- baseline build / test,
- safety enforcement w BUY path,
- spójność configu,
- jawny WAL contract,
- rollout runbook,
- economics / accounting.

---

# Program wykonawczy

## PR-1 — `runtime-baseline-and-execution-ssot`

### Twardy cel PR

Przywrócić zielony baseline oraz uczynić execution semantics jednoznacznym kontraktem runtime, bez mieszania legacy aliasów i ukrytych defaultów.

### Dlaczego ten PR musi być pierwszy

Bez zielonego baseline i bez jawnego SSOT dla execution mode nie da się uczciwie ocenić, co system faktycznie robi. Każdy dalszy rollout oparty na niejednoznacznym configu jest z definicji ryzykowny.

### Pliki obowiązkowe

- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-brain/tests/runtime_strategy_tests.rs`
- wszystkie miejsca inicjalizacji typów dotkniętych nowymi polami
- `ghost-launcher/src/config.rs`
- `config.toml`
- ewentualnie testy / docs związane z config semantics

### Zakres implementacyjny

#### 1. Naprawić compile/test blockers

1. Uzupełnić wszystkie inicjalizatory `GatekeeperBuyLog` o nowe pola `iwim_snap_*`.
2. Uzupełnić wszystkie inicjalizatory `CandidatePool` o brakujące pola takie jak `event_ts_ms` i `semantic`.
3. Przejść przez miejsca, w których podobne kontrakty typów mogły zostać rozjechane po ostatnich zmianach.

#### 2. Uczynić `[execution].execution_mode` rzeczywistym SSOT

1. Upewnić się, że loader configu nie opiera się już semantycznie na niejawnych defaultach typu "brak sekcji = live".
2. Gdy config rolloutowy ma działać produkcyjnie, `execution_mode` musi być ustawiony jawnie.
3. `dry_run` ma pozostać co najwyżej jako legacy alias z ostrym warningiem, a nie jako równoległe źródło prawdy.

#### 3. Ujednoznacznić relację `execution_mode` <-> `trigger.entry_mode`

1. Zdefiniować i wymusić legalne pary:
   - `paper + shadow_only`
   - `dual + live_and_shadow`
   - `live + live`
2. Wykrywać i raportować konfiguracje niespójne.
3. Fail-fast przy profilach ewidentnie sprzecznych w produkcyjnym starcie.

#### 4. Przygotować jawne profile rolloutowe

1. W `config.toml` lub w docelowych plikach konfiguracyjnych przygotować czytelne, jawne profile:
   - burn-in paper,
   - dual mikro-live,
   - future live.
2. Profil bieżący nie może już polegać na ukrytym dziedziczeniu zachowania po defaultach.

### Czego nie wolno zrobić w tym PR

- Nie wpinać jeszcze bulkheada do BUY path.
- Nie ruszać jeszcze WAL activation semantics.
- Nie wprowadzać live rollout changes.
- Nie rozbudowywać paper lifecycle.

### Testy i walidacja obowiązkowa

1. `cargo test --workspace --no-run` na środowisku zdolnym domknąć build.
2. Minimum: targeted builds/testy dla crate'ów dotkniętych kontraktami typów.
3. Testy / walidacja config loadera dla spójnych i niespójnych par `execution_mode` + `entry_mode`.

### Merge gate

PR nie może zostać scalony, jeśli:

- repo nadal ma compile blockers wynikające z rozjechanych typów,
- config pozwala na produkcyjny start z niejednoznacznym execution profile,
- system nadal milcząco mapuje rollout do live bez jawnej deklaracji.

---

## PR-2 — `authoritative-buy-safety-bulkhead`

### Twardy cel PR

Wpiąć bulkhead safety w kanoniczną ścieżkę BUY oraz uczynić limity kapitałowe realnym guardem runtime, a nie tylko opisaną intencją w configu.

### Dlaczego ten PR jest krytyczny

Dopóki `ghost-launcher/src/components/trigger/safety.rs` nie jest używany przez rzeczywistą ścieżkę BUY, system nie ma produkcyjnego hamulca bezpieczeństwa. To dyskwalifikuje każdy live rollout.

### Pliki obowiązkowe

- `ghost-launcher/src/components/trigger/safety.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/config.rs`
- `config.toml`
- metryki / eventy rejection path, jeśli istnieją odpowiednie miejsca

### Zakres implementacyjny

#### 1. Egzekwować emergency floor i buffer przed BUY

1. W kanonicznym BUY path odczytać saldo payera.
2. Zbudować `SafetyConfig` na podstawie runtime configu.
3. Uruchomić `check_emergency_floor()` i `validate_trade()` przed zbudowaniem i wysłaniem BUY.
4. Odrzucić BUY, jeżeli trade narusza floor, buffer albo safe size.

#### 2. Egzekwować rzeczywisty safe position sizing

1. Nie opierać się wyłącznie na `max_position_size_sol`.
2. Rzeczywisty size ma być:
   - ograniczony przez `max_position_size_sol`,
   - ograniczony przez dostępne saldo po odjęciu floor i buffer,
   - jawnie logowany.

#### 3. Wymusić bezpieczny profil pierwszych rolloutów

1. Dla paper burn-in i dual mikro-live przygotować bezpieczne wartości startowe:
   - `max_concurrent_positions = 1`,
   - niezerowy `emergency_floor_sol`,
   - niezerowy `position_size_buffer_sol`,
   - dust-sized ekspozycja.
2. Nie polegać na ręcznym "później sobie ustawimy".

#### 4. Uczynić rejection path diagnostycznym

1. Każde odrzucenie BUY przez safety ma mieć jawny powód.
2. Powód ma być widoczny w logach i, jeśli repo ma odpowiednią ścieżkę, także w eventach/metrikach.
3. Rejection by safety nie może wyglądać jak błąd nieznanego pochodzenia.

#### 5. Domknąć limit pozycji

1. Zweryfikować, gdzie w runtime realnie egzekwowany jest `max_concurrent_positions`.
2. Jeżeli dziś tylko jest logowany albo opisywany, podpiąć realny guard.
3. Guard ma działać zarówno dla paper burn-in, jak i dla dual rollout.

### Czego nie wolno zrobić w tym PR

- Nie wdrażać jeszcze nowych preflightów i skryptów operacyjnych.
- Nie ruszać jeszcze secret handling.
- Nie przechodzić do live entry enable.

### Testy i walidacja obowiązkowa

1. Testy jednostkowe safety dla floor, buffer, trade size.
2. Testy triggera / integracyjne dla:
   - odrzucenia przy zbyt małym saldzie,
   - odrzucenia przy naruszeniu buffer,
   - poprawnego przycięcia amount,
   - odrzucenia przy limicie pozycji.
3. Walidacja logów / eventów rejection path.

### Merge gate

Nie wolno scalć PR-a, jeśli da się wykonać BUY z naruszeniem:

- emergency floor,
- position buffer,
- max concurrent positions,
- albo jeśli runtime nadal nie podaje jawnego powodu rejection.

---

## PR-3 — `paper-lifecycle-accounting-and-compare-trace`

### Twardy cel PR

Przekształcić istniejący paper lifecycle z "technicznie istniejącego adaptera" w rolloutowy etap obowiązkowy, który daje pełny i wiarygodny ślad pozycji od entry do exit oraz podstawę pod economics review.

### Dlaczego ten PR jest potrzebny

Shadow buy telemetry nie odpowiada jeszcze na pytanie, czy post-buy lifecycle jest poprawny, czy zarządzanie pozycją działa i czy system potrafi zamknąć pozycję w sposób mierzalny.

### Pliki obowiązkowe

- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-brain/src/execution/paper_lifecycle.rs`
- `ghost-brain/src/events/*`
- `ghost-brain/src/execution/paper.rs`
- `ghost-brain/src/quotes/*`
- event writer / JSONL output config powiązany z paper lifecycle

### Zakres implementacyjny

#### 1. Uczynić paper lifecycle pełnym etapem dowodowym

1. Każda pozycja paper ma mieć czytelny lifecycle:
   - candidate,
   - entry submitted,
   - entry filled,
   - position opened,
   - management ticks / decisions,
   - exit submitted,
   - exit filled,
   - position closed.
2. Zdarzenia te mają być łatwo korelowalne po `candidate_id` / `position_id`.

#### 2. Dodać jawne pola accountingowe

1. Po zamknięciu paper position emitować jawne pola:
   - entry value,
   - exit value,
   - gross pnl,
   - net pnl placeholder lub jawne koszty składowe,
   - duration,
   - close reason.
2. Nie zostawiać analizy economics do domysłów z surowych eventów.

#### 3. Zbudować compare trace shadow <-> paper

1. Dla kandydatów przechodzących przez paper burn-in zapewnić możliwość korelacji:
   - decision,
   - shadow buy result,
   - paper lifecycle outcome.
2. W praktyce oznacza to wspólny identyfikator / łatwo łączalne pola między logami.

#### 4. Urealnić paper as pre-live gate

1. Paper lifecycle ma być uruchamiany w sposób przewidywalny dla rollout profile.
2. W `dual` nie może się okazać, że paper działa "trochę inaczej" niż w burn-in.
3. Event output ma być stabilny i gotowy do analizy sesyjnej.

### Czego nie wolno zrobić w tym PR

- Nie budować jeszcze pełnego live position engine.
- Nie mieszać tego PR-a z WAL / preflight / secrets.
- Nie próbować rozwiązywać economics samą heurystyką bez jawnych event fields.

### Testy i walidacja obowiązkowa

1. Test lifecycle: entry -> fill -> exit -> close.
2. Testy event emission dla pozycji paper.
3. Test, że pozycja zamknięta ma komplet podstawowych danych accountingowych.
4. Test lub walidacja, że paper output da się skorelować ze shadow trace.

### Merge gate

PR nie może zostać scalony, jeśli po paper run:

- nie da się ustalić pełnego lifecycle pozycji,
- zamknięta pozycja nie ma jawnych danych do PnL review,
- trace shadow/paper pozostaje niekorelowalny operacyjnie.

---

## PR-4 — `durability-preflight-and-production-runbook`

### Twardy cel PR

Zamknąć kontrakt durability i startupu oraz zbudować produkcyjny preflight / runbook, tak aby rollout nie zależał od pamięci operatora.

### Dlaczego ten PR jest konieczny

Na dziś WAL jest aktywowany env-em, a operacyjny start wygląda zbyt ręcznie. To jest akceptowalne dla eksperymentu, ale nie dla kontrolowanego burn-in i nie dla live capital path.

### Pliki obowiązkowe

- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/wal_recovery.rs`
- `ghost-launcher/tests/wal_startup_recovery.rs`
- `docs/RUNBOOK_HOT_PATH_METRICS.md`
- nowy skrypt preflight w `scripts/`
- ewentualny osobny runbook rolloutowy w `docs/`

### Zakres implementacyjny

#### 1. Ujednoznacznić aktywację WAL

1. Zdecydować i wdrożyć jeden kontrakt:
   - albo WAL aktywowany przez config,
   - albo WAL aktywowany przez env, ale jawnie logowany i fail-fast walidowany.
2. Nie zostawiać operatorowi domyślania się, czy wpis w configu coś naprawdę włącza.

#### 2. Dodać startup validation dla durability

1. Przed startem runtime sprawdzać:
   - dostępność katalogów,
   - prawa zapisu,
   - zgodność konfiguracji snapshot/WAL,
   - jawne logowanie aktywnego durability mode.

#### 3. Zbudować production preflight

Preflight ma sprawdzać co najmniej:

- spójność configu execution/trigger,
- dostępność keypair,
- minimalne saldo względem floor + buffer,
- poprawność RPC / gRPC,
- poprawność Jito endpoint, gdy włączony,
- gotowość katalogów WAL/snapshot/log,
- wolny port metryk,
- potwierdzenie, że build/test baseline jest zaakceptowany dla rollout revision.

#### 4. Spisać autorytatywny runbook

Runbook ma zawierać:

- start,
- stop,
- restart,
- recovery check,
- rollback / abort,
- obserwowane metryki,
- kill-switch conditions.

### Czego nie wolno zrobić w tym PR

- Nie zmieniać jeszcze economics logic.
- Nie uruchamiać live.
- Nie mieszać runbooka z cleanupem paper lifecycle.

### Testy i walidacja obowiązkowa

1. Testy recovery / restart semantics.
2. Walidacja preflightu na:
   - configu poprawnym,
   - configu niespójnym,
   - brakującym WAL dir / keypair / endpoint.
3. Przejście przez runbook w warunkach testowych.

### Merge gate

PR nie może zostać scalony, jeśli operator nadal nie ma jednej, autorytatywnej procedury:

- startu,
- restartu,
- potwierdzenia recovery,
- abortu rolloutu.

---

## PR-5 — `secret-hygiene-and-rollout-profiles`   // WSTRZYMANO I PRZESUNIĘTO NA KONIEC PLANU!

### Twardy cel PR

Oddzielić repo od sekretów i przygotować czyste profile rolloutowe dla:

- paper burn-in,
- dual mikro-live,
- docelowego live.

### Dlaczego ten PR ma własny zakres

Sekrety i rollout profile to nie są drobiazgi konfiguracyjne. To warunek wejścia do czegokolwiek, co używa realnych endpointów i realnego walleta.

### Pliki obowiązkowe

- `config.toml`
- profile / przykładowe configi rolloutowe, jeśli repo ich używa
- `.gitignore`, jeśli potrzebne
- dokumentacja operatora dotycząca sekretów
- ścieżki / narzędzia pomocnicze do secret loading, jeśli repo ich potrzebuje

### Zakres implementacyjny

#### 1. Przestać traktować repo jak secret store

1. Upewnić się, że wallet/keypair nie jest trzymany w trackowanym miejscu workflow.
2. Endpoint credentials nie mogą pozostać na twardo w głównym configu produkcyjnym.
3. Rozdzielić:
   - repo code,
   - runtime config,
   - secret material,
   - funding wallet.

#### 2. Przygotować profile rolloutowe

1. Profil `paper-burnin`:
   - `execution_mode = paper`
   - `entry_mode = shadow_only`
   - 1 pozycja
   - dust size
   - niezerowy floor i buffer
2. Profil `dual-micro-live`:
   - `execution_mode = dual`
   - `entry_mode = live_and_shadow`
   - 1 pozycja
   - osobny wallet
3. Profil `future-live`:
   - przygotowany, ale nieużywany do czasu zakończenia PR-7.

#### 3. Udokumentować politykę walletów

1. Osobny wallet dla rolloutu.
2. Osobny mały funding.
3. Brak mieszania z innymi aktywami.
4. Jasna procedura rotacji / wymiany po cleanupie.

### Czego nie wolno zrobić w tym PR

- Nie uruchamiać jeszcze dual mikro-live.
- Nie zwiększać size.
- Nie zostawiać "tymczasowo" sekretów w repo.

### Testy i walidacja obowiązkowa

1. Ręczna walidacja, że tracked config nie zawiera secret material.
2. Preflight dla wszystkich rollout profiles.
3. Walidacja, że profile startują zgodnie z oczekiwaną semantyką.

### Merge gate

PR nie może zostać scalony, jeśli:

- główny workflow nadal zakłada keypair w repo-adjacent path,
- tracked config dalej trzyma realne sekrety,
- profile rolloutowe nie są jednoznaczne i odtwarzalne.

---

## PR-6 — `paper-burnin-operations-and-go-no-go-gates`

### Twardy cel PR

Przekuć paper burn-in w formalny etap operacyjny z twardymi gate'ami, artefaktami sesji i jednoznacznym warunkiem przejścia do mikro-live.

### Dlaczego to osobny PR

Po PR-1..PR-5 system ma być gotowy nie do live, tylko do **uczciwego paper burn-in**. Ten etap musi być zamknięty jako program operacyjny, nie jako nieformalna seria odpaleń.

### Pliki obowiązkowe

- skrypty / tooling do analizy artefaktów rolloutowych
- dokumentacja burn-in gates
- ewentualne lekkie rozszerzenia event/log output, jeśli potrzebne do raportów
- profile rolloutowe przygotowane w PR-5

### Zakres implementacyjny

#### 1. Zdefiniować artefakty obowiązkowe paper burn-in

Minimalny zestaw:

- `logs/shadow_run/buys.jsonl`
- `logs/decisions.jsonl`
- `datasets/events/*`
- log systemowy i metryki hot path
- raport sesyjny spinający decision -> shadow -> paper position

#### 2. Zdefiniować twarde burn-in gates

Do przejścia dalej wymagane:

1. brak czerwonych correctness issues po starcie i restartach,
2. brak naruszeń safety,
3. brak recovery surprises,
4. brak systemowego event bus lag,
5. pełny lifecycle pozycji paper,
6. brak katastrofalnej divergence shadow vs paper,
7. economics paper/shadow nie wyglądają trwale fatalnie po kosztach i failure modes.

#### 3. Zdefiniować kill-switch conditions dla burn-in

Burn-in ma zostać natychmiast wstrzymany, jeśli:

- runtime gubi recovery contract,
- safety zaczyna często odrzucać poprawne nominalnie setupy z powodu driftu configu,
- event bus lag narasta,
- shadow/paper trace przestaje być korelowalny,
- logi wskazują nieautoryzowane side effects,
- pojawia się duplicate fire na ten sam mint / candidate path.

#### 4. Wprowadzić formalny raport kończący burn-in

Raport ma odpowiadać na pytania:

- czy system poprawnie przechodzi od decyzji do zamkniętej paper pozycji,
- czy economics po kosztach i założeniach execution nie są ewidentnie ujemne,
- czy runtime nadaje się do `dual + live_and_shadow`.

### Czego nie wolno zrobić w tym PR

- Nie włączać jeszcze realnych BUY.
- Nie traktować dobrych pojedynczych sesji jako wystarczającego dowodu.
- Nie pomijać raportu końcowego.

### Testy i walidacja obowiązkowa

1. Suchy przebieg runbooka paper burn-in.
2. Walidacja artefaktów dla sesji testowej.
3. Potwierdzenie, że da się wyprodukować raport go/no-go na podstawie istniejących danych.

### Merge gate

PR nie może zostać scalony, jeśli po jego wdrożeniu nadal nie istnieje formalny sposób stwierdzenia:

- "paper burn-in zaliczony",
- albo "paper burn-in niezaliczony i wracamy do researchu".

---

## PR-7 — `dual-micro-live-and-net-pnl-proof`

### Twardy cel PR

Przygotować i zamknąć pierwszy **kontrolowany mikro-live** jako etap dowodowy, nie skalujący, z równoległym shadow trace i z pełnym liczeniem netto wyniku.

### Dlaczego to ostatni PR

To nie jest PR od "wreszcie kupmy coś naprawdę". To PR od:

- minimalnej ekspozycji,
- maksymalnego porównania,
- jawnego kill-switchu,
- uczciwego wyniku netto.

### Pliki obowiązkowe

- profile `dual + live_and_shadow`
- event/log accounting powiązany z realnym execution
- dokumentacja mikro-live rollout
- ewentualne skrypty / raporty net PnL

### Zakres implementacyjny

#### 1. Ustawić kanoniczny profil pierwszego mikro-live

Minimalny profil:

- `execution_mode = dual`
- `entry_mode = live_and_shadow`
- `max_concurrent_positions = 1`
- dust-sized `max_position_size_sol`
- osobny rollout wallet
- aktywne safety, WAL, preflight, runbook

#### 2. Wprowadzić obowiązkowe porównanie live vs shadow

Każdy realny BUY w tym etapie musi dawać:

- realny execution trace,
- równoległy shadow trace,
- możliwość policzenia divergence fill quality.

#### 3. Liczyć realny wynik netto

Każdy trade ma kończyć się wynikiem:

- wejście,
- wyjście,
- fee,
- Jito tip,
- slippage / execution loss,
- wynik netto,
- close reason,
- operational anomalies.

#### 4. Zdefiniować kill-switch dla mikro-live

Natychmiastowy abort przy:

- dużej i powtarzalnej divergence shadow vs live,
- nieoczekiwanym duplicate BUY,
- utracie recovery contract,
- braku WAL przy oczekiwanej aktywacji,
- zejściu walleta do emergency floor,
- economics zjadanych przez fees/tipy,
- nieautoryzowanych side effects.

#### 5. Zdefiniować końcową decyzję po mikro-live

Po zakończeniu etapu wynik ma być binarny:

1. system nadaje się do dalszego, ostrożnego rozszerzania,
2. system nie nadaje się jeszcze do skalowania i wraca do poprawy selekcji / execution / lifecycle.

### Czego nie wolno zrobić w tym PR

- Nie zwiększać pozycji ponad dust-size.
- Nie przełączać systemu na `live + live`.
- Nie utożsamiać samego wykonania realnego BUY z sukcesem rolloutu.

### Testy i walidacja obowiązkowa

1. Preflight dla profilu dual.
2. Kontrolowany dry rehearsal całego runbooka.
3. Potwierdzenie, że po mikro-live dostępny jest pełny raport:
   - shadow vs live,
   - net PnL,
   - operational loss,
   - recommendation: continue / abort.

### Merge gate

PR nie może zostać uznany za zakończony, jeśli system po mikro-live nie umie odpowiedzieć:

- czy wynik netto po kosztach był dodatni czy ujemny,
- czy divergence execution jest akceptowalna,
- czy runtime zachował correctness i safety pod realnym kapitałem.

---

# Kolejność realizacji

Realizacja ma iść dokładnie tak:

1. PR-1 `runtime-baseline-and-execution-ssot`
2. PR-2 `authoritative-buy-safety-bulkhead`
3. PR-3 `paper-lifecycle-accounting-and-compare-trace`
4. PR-4 `durability-preflight-and-production-runbook`
5. PR-5 `secret-hygiene-and-rollout-profiles`
6. PR-6 `paper-burnin-operations-and-go-no-go-gates`
7. PR-7 `dual-micro-live-and-net-pnl-proof`

Nie wolno przeskakiwać kolejności.

---

# Definicja sukcesu programu

Program uznajemy za zakończony sukcesem tylko wtedy, gdy po PR-7 prawdziwe są jednocześnie wszystkie poniższe zdania:

1. repo ma zielony i wiarygodny baseline,
2. execution semantics są jawne i spójne,
3. bulkhead działa w realnej ścieżce BUY,
4. WAL/recovery są aktywne i potwierdzone,
5. paper burn-in daje pełny lifecycle oraz economics trace,
6. dual mikro-live przechodzi bez naruszeń safety,
7. dostępny jest uczciwy net PnL po wszystkich kosztach,
8. na końcu można podjąć świadomą decyzję:
   - kontynuować bardzo ostrożnie,
   - albo wrócić do researchu bez dalszego spalania kapitału.

Jeżeli choć jeden z tych punktów pozostaje nieudowodniony, projekt nie powinien przechodzić do większego live rolloutu.

---

# Najważniejsza zasada końcowa

Shadow run już nie ma udowadniać, że pipeline żyje. To zostało w dużej mierze potwierdzone.

Od tego momentu program prac ma udowodnić trzy rzeczy:

1. system potrafi bezpiecznie utrzymać i zamknąć pozycję w paper,
2. system potrafi wejść w mikro-live bez utraty correctness i safety,
3. po wszystkich kosztach pozostaje sensowna ekonomika, a nie tylko ładny wykres działania runtime.

To jest właściwy próg przejścia z "stabilnego shadow runtime" do "rzeczywistego programu dojścia do live".
