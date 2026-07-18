# ADR-8D: HET-PM V2 — Schema V3 lattice, frozen validation lock i bounded admission shutdown

Data: 2026-07-17

Typ: ADR-8D / PR #73 promotion-evidence prerequisite / observe-only telemetry

Status: Accepted and materialized. Contract został zablokowany po buildzie reviewed
release binary z `7e162a90846a0425cc3ed01f90bcf2fb52d39c71`; criteria zapisują jej
SHA-256 `8f38ee7879f4c8ce58b43c3757b4fe1cd09d4b398a07e56d99165c690e6a3804`.
Pozostaje niepromowalny wyłącznie dlatego, że nie istnieją jeszcze dwa wymagane
prospective validation runy ani source-recomputed promotion artifact.

## D1. Problem

Focused re-review PR #73 wykazał trzy krytyczne luki producer/validation contractu:

1. jeden comparison record PR-A zawierał tylko zwycięzcę hierarchii; przy jednoczesnym Crash i Trailing nie dało się ustalić niższego, selectively-promoted candidate;
2. criteria mieszały version narzędzia z version rzeczywistej polityki i próbowały wymagać jednego pełnego run-config hash dla dwóch z definicji różnych runów;
3. progi ekonomiczne mogły dopuszczać istotne pogorszenie, zaś stability nie wymuszało wyniku per-run × per-promoted-gate.

Drugorzędny problem runtime: admission writer nie blokował aktywnego handoffu, ale jego shutdown wykonywał nieograniczony `JoinHandle::join()` na async shutdown path.

## D2. Zakres decyzji

Decyzja dotyczy wyłącznie obserwacyjnego producenta PR-A i prerequisite evidence PR #73.

Poza zakresem pozostają:

- authority cutover PR B;
- jakakolwiek zmiana V1 proposal/apply/terminal/capacity ownership;
- interpretowanie obecnego r2a jako finalnego validation runu;
- uruchomienie prospective runu i jakakolwiek promocja authority.

## D3. Root cause

`v2_winning_gate` i `v2_suppressed_gates_mask` są wystarczające dla obserwacji istniejącej, sztywnej hierarchii, lecz nie dla kontrfaktycznej oceny selective authority. Maska nie zawiera per-gate quote outcome ani executable return, więc nie pozwala ocenić Trailingu ukrytego przez observe-only Crash.

Pełny SHA-256 pliku konfiguracji jest celowo wrażliwy na operational fields (run ID, ścieżki, porty). Nie może jednocześnie być shared behavioural identity dwóch niezależnych runów.

## D4. Decyzja

1. PR-A comparison payload przechodzi na schema V3, zachowując HET policy version 2.
   - Record ma deterministyczny, uporządkowany `v2_gate_evaluations` dla Crash, HardLoss, ExecutableTrailing, VitalityDecay i AbsoluteMaxHold.
   - Każda pozycja zawiera per-gate prequote, quote status, final decision i optional executable return.
   - `v2_winning_gate` nadal opisuje obserwowanego zwycięzcę pierwotnej hierarchii; lattice jest dodatkiem evidence-only.
   - Lattice używa tego samego immutable `PostBuySnapshotBundle` i już rozwiązanego quote cell. Nie tworzy quote I/O, proposal, apply ani terminalu.

2. Promotion tool czyta promoted candidates wyłącznie z Schema-V3 lattice. W starszych fixture'ach możliwy jest jawny legacy branch testowy, ale production structural analyzer odrzuca schema V2.

3. Criteria rozdzielają:
   - `criteria_version` / `tool_version` / `comparison_schema_version`;
   - rzeczywiste `policy_id` i `policy_version=2` producenta;
   - exact `run_config_content_hash` dla każdego prospective `run_id`;
   - wspólny `normalized_behavioral_config_hash` obejmujący konfigurację wpływającą na entry/HET/V1/TimeStop/Crash/quote/replay/capacity/sampling.

4. `lock-criteria` jest jedynym dozwolonym sposobem zmiany template z `calibration_pending` na `locked`. Komenda wymaga:
   - reviewed commit SHA lub jego jednoznacznego skrótu, kanonizowanego przez Git
     do pełnego commita;
   - istniejącej release binary i jej SHA-256;
   - brain configu;
   - co najmniej dwóch konkretnych run configów;
   - identycznego znormalizowanego behavioural hash;
   - dwóch różnych exact run-config hashy.
   - potwierdzenia, że commit istnieje i jest ancestor aktualnego PR head.

   `evaluate` i source-recomputing `validate` odmawiają pracy z criteria niebędącym `locked`.

5. Bez osobnej, uprzednio zaakceptowanej kalibracji ekonomicznej obowiązuje conservative non-inferiority:
   - wszystkie mean/tail/CVaR/cost/trimmed deltas dla promoted gate są `>= 0`;
   - sample są co najmniej 100 candidates / 80 matched globalnie i 40 matched w każdym z dwóch runów; 80 observations daje co najmniej 16 obserwacji w CVaR 20%, a 40 daje co najmniej 8 per run;
   - top-3 positive improvement share jest ograniczony do 0.35;
   - false-early proxy jest ograniczone do 0.10;
   - executable continuation coverage jest `>= 0.8`, route availability `== 1.0`, a censor/economic-join loss jest 0.

   Są to ochronne floors, nie twierdzenie o estymacji EV. Ich poluzowanie wymaga nowej wersji criteria i nowego prospective validation protocol, nie ręcznej edycji w trakcie runów.

6. Admission shutdown wykonuje join oraz health write przez `spawn_blocking` z twardym budżetem 100 ms. Timeout ustawia `shutdown_complete=false`; runtime nie czeka na potencjalnie zablokowany filesystem, a analyzer nie może zaliczyć takiego evidence.

## D5. Konsekwencje

- Rzeczywiste same-tick Crash + Trailing jest mierzalne jednym payloadem i nie wymaga sztucznego duplikowania comparison rows.
- Późniejszy PR B może wybierać authoritative winnera z pełnej lattice po odfiltrowaniu gate'ów bez authority, bez niejawnej promocji CrashGuarda.
- Stare r2a/schema-V2 pozostaje diagnostic/calibration i nie może przejść production evidence evaluation.
- Zablokowane criteria wiążą konkretny reviewed source commit, binarkę,
  brain config, dwa exact run-configi i wspólny behavioural contract.
- Writer timeout nie blokuje shutdown, lecz brak durable health pozostaje fail-closed dla Gate 1.

## D6. Implementacja

Zmiany obejmują:

- `ghost-brain/src/guardian/post_buy/exit_policy_v2.rs`;
- `ghost-brain/src/guardian/post_buy/engine.rs`;
- `scripts/het_pm_v2_analysis.py`;
- `scripts/het_pm_v2_promotion_gate_v1.py`;
- oba testy Python promotion/analyzer;
- `PLANS/DO_REALIZACJI/HET_PM_V2_PROMOTION_CRITERIA_V1.json`;
- oba prospective run-config templates z identycznym behavioural sampling version;
- `ghost-launcher/src/components/post_buy_runtime.rs`.

## D7. Weryfikacja

Wymagane lokalne dowody:

```text
python3 scripts/test_het_pm_v2_promotion_gate_v1.py
python3 scripts/test_het_pm_v2_analysis.py
cargo test -p ghost-brain guardian::post_buy::exit_policy_v2::tests::same_snapshot_gate_lattice_retains_trailing_beneath_observed_crash --lib
cargo test -p ghost-launcher components::post_buy_runtime::tests::shadow_terminal_watcher_writes_admission_terminal_release --lib
cargo fmt --check
```

Test Rust materializuje prawdziwy CrashGuard prequote z V1 i prawdziwy Trailing prequote z tego samego snapshotu, a następnie serializuje jeden comparison record z oboma per-gate outcomes.

## D8. Ryzyka i następne kroki

1. Release binary z reviewed source i `lock-criteria` zostały już materializowane
   oraz committed w PR #73; ich identities są polami criteria, a nie opisem
   operatorskim.
2. Następnym krokiem są dokładnie dwa nowe, niezależne prospective validation
   runy bez dalszego strojenia.
3. PR B nadal wymaga osobnego implementation review dla deploy drain,
   `PendingExitProposal`/pending terminal, selective hierarchy i
   `AbsoluteMaxHold` ponad blocked gate.
