# ADR-8D: HET Position Manager V2 PR A — bounded sidecar i izolacja timingowa V1

Status: `IMPLEMENTED / LOCAL VALIDATION COMPLETE / PR #71 DRAFT`

Typ: ADR-8D / review remediation / aktywny shadow post-buy / async persistence / evidence integrity

Data: 2026-07-16

Repo: `smahacfel/Gho`

Branch: `agent/het-pm-v2-pr-a`

Base SHA: `18d94b0cc5a226496a5ac2bc616e7488a7f78d5d`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`, wyłącznie PR A, w szczególności §14–§15.

Powiązane ADR:

- `ADR_8D_HET_PM_V2_PR_A_OBSERVE_ONLY_20260716.md`;
- `ADR_8D_HET_PM_V2_PR_A_REVIEW_REMEDIATION_20260716.md`.

Poziom ryzyka: `MEDIUM-HIGH` — zmiana dotyka aktywnego shadow post-buy loop,
terminal persistence ordering i capacity release. Nie zmienia V1 policy, nie
dodaje V2 authority, nie aktywuje live execution i nie tworzy drugiego
terminal truth.

## 1. Problem

Czwarty focused re-review PR #71 wykazał, że HET-PM był logicznie
observe-only, lecz jego sidecar wykonywał synchroniczne filesystem I/O w tym
samym tasku Tokio co V1 authority:

```text
OpenOptions::open
-> write_all(record)
-> write_all(newline)
-> flush
```

Na ticku nieterminalnym wolny lub zawieszony filesystem mógł przedłużyć tick i
przy `MissedTickBehavior::Skip` opóźnić następną ocenę V1. Na ticku terminalnym
ten sam zapis wykonywał się przed canonical terminal appendem, więc mógł
opóźnić usunięcie pozycji oraz zwolnienie capacity. Natychmiastowy błąd I/O był
fail-open, ale czas oczekiwania nie miał górnej granicy.

To stanowiło wpływ ekonomiczny przez timing, mimo braku bezpośredniej mutacji
policy lub lifecycle przez V2.

## 2. Decyzja

Zastosowano wariant bounded writer acknowledgement:

```text
prevalidated comparison bytes
-> single bounded sync_channel
-> dedicated HET sidecar OS writer thread
-> open/write/flush poza Tokio authority taskiem
```

Własność pozostaje jednoznaczna:

- V1 jest jedynym proposal/apply/terminal/capacity ownerem;
- jeden primary HET writer jest jedynym producentem comparison sidecara;
- probe monitor nie konstruuje drugiego HET writera;
- HET writer nie uczestniczy w `canonical_committed()`;
- sidecar nie jest terminal SSOT.

## 3. Nonterminal contract

Nieterminalny tick wykonuje wyłącznie:

```text
validate + serialize locally
-> try_send(prevalidated bytes)
-> immediate return
```

Nie oczekuje na:

- otwarcie pliku;
- zapis;
- flush;
- acknowledgement writera.

Pełna kolejka albo zamknięty writer powodują drop observer row z typed
structured diagnostic i atomowym counterem. Nie powodują retry w authority
tasku, nie blokują następnego ticku i nie zmieniają pozycji.

## 4. Terminal contract

Terminal comparison nadal pochodzi z oryginalnego pre-mutation snapshotu i
jest przypięty do istniejącego `PendingTerminalCommit`. Przy pierwszej próbie
terminal persistence:

```text
try_send(prevalidated bytes + oneshot ack)
-> wait at most terminal_write_budget_ms
-> Written
   albo typed Skipped
-> operational/canonical terminal append
-> cleanup + capacity release
```

Typed degradacje writera obejmują:

- `writer_not_configured`;
- `writer_unavailable`;
- `writer_queue_full`;
- `writer_queue_closed`;
- `writer_timed_out`;
- `writer_io_failed`.

Timeout ack jest twardy i nie przekracza skonfigurowanego budżetu. Gdy request
pozostaje jeszcze w kolejce, worker sprawdza zamknięcie acknowledgement przed
rozpoczęciem I/O i pomija anulowane zlecenie. Jeżeli syscall rozpoczął się przed
timeoutem, system nie próbuje go przerywać w sposób nieprzenośny; canonical V1
commit i tak nie czeka dłużej niż budżet.

## 5. Konfiguracja i identity

Do `[post_buy_guardian.het_pm_v2]` dodano backward-compatible pola objęte
`#[serde(default)]`:

```toml
writer_queue_capacity = 256
terminal_write_budget_ms = 25
```

Startup validation wymaga:

```text
1 <= writer_queue_capacity <= 4096
1 <= terminal_write_budget_ms <= 100
```

Oba pola należą do HET config hash, ponieważ zmieniają sampling/backpressure
contract evidence. Status startupowy i structured launcher log pokazują ich
effective values.

## 6. Semantyka receiptu

`v1_authority_receipt.terminal_commit_status` oznacza stan w chwili finalizacji
comparison, nie finalny rezultat późniejszego canonical joinu. W kodzie pole ma
jawny komentarz kontraktowy. Ostateczny terminal outcome nadal ustala się przez
join z canonical `TERMINAL_TRUTH` po `comparison_id` i `action_id`.

Nie zmieniono sidecar schema ani analyzer contractu w tej korekcie.

## 7. Fault injection i dowody

Testowy stalled writer trzyma bounded receiver bez konsumenta. Pozwala bez
zależności od szybkości prawdziwego dysku deterministycznie wymusić:

- zapełnienie kolejki;
- brak terminal acknowledgement;
- terminal timeout;
- zachowanie correlation ID;
- dalszy canonical commit i capacity release.

Dodane testy akceptacyjne:

```text
slow_nonterminal_het_writer_does_not_delay_next_v1_tick
full_het_writer_queue_does_not_block_v1_evaluation
slow_writer_cannot_trigger_missed_authority_tick
terminal_het_writer_timeout_marks_skipped_and_continues_canonical_commit
terminal_het_writer_timeout_does_not_delay_capacity_beyond_configured_budget
terminal_writer_timeout_preserves_comparison_id_and_typed_skip_reason
writer_queue_and_terminal_budget_are_bounded_at_startup
```

Istniejące testy terminal comparison, canonical retry, writer I/O failure,
same-snapshot receipt i analyzer schema pozostają obowiązkowe.

## 8. Inwarianty zachowane

- ten sam pre-mutation snapshot bundle dla V1 i V2;
- V1-only proposal/apply/terminal/capacity authority;
- brak V2 evaluation na terminal retry;
- exactly-once enqueue attempt przypiętego terminal comparison;
- typed `Skipped` nie blokuje canonical appendu;
- sidecar failure nie zatrzymuje terminal notification ani capacity release;
- brak blocking filesystem I/O HET w Tokio authority tasku;
- bounded memory przez stałą pojemność kolejki;
- brak nowej live execution ścieżki;
- brak zmian Gate 4/5 i brak deklarowania burn-in evidence bez rzeczywistego runu.

## 9. Rollback

Rollback jest lokalny:

1. wyłączyć `[post_buy_guardian.het_pm_v2].enabled`;
2. usunąć bounded writer fields i worker;
3. zachować V1 authority oraz istniejący canonical terminal flow;
4. nie używać niepełnego sidecara do promotion decision.

Wyłączenie HET nie zmienia V1 lifecycle ani terminal/capacity ownership.

## 10. Lokalna walidacja

Zielone kontrole przed publikacją:

```text
cargo fmt --all -- --check
cargo test -p ghost-brain guardian::post_buy --lib
cargo test -p ghost-brain --test ghost_brain_config_load_test
cargo test -p ghost-brain writer --lib
cargo test -p ghost-launcher components::post_buy_runtime::tests::pr_a_ --lib
```

Dokładny diff-scoped Clippy względem base PR jest uruchamiany na finalnym,
zacommitowanym headzie przed push. Nie uruchamiano shadow burn-in ani analiz
Gate 4/5; ta korekta pozostaje częścią implementacji PR A i nie stanowi
evidence do promocji PR B.
