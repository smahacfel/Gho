# PR-RTP-A0: Right-Tail Preservation Offline Proof

Data: `2026-06-28`

Final verdict: `RTP_DIAGNOSTIC_ONLY / NO_RUNTIME`

R51 decision: `NO_GO_FOR_R51`

## Granica runtime

Ten raport jest offline-only. Nie zatwierdza zmian runtime, `shadow_close_only`, active close, BUY/REJECT, Gatekeeper policy, selector runtime, `alpha_31100`, XGBoost ani TX/Jito/live path. Nie dodano nowych masek TSV2 ani nowych progow runtime. Surowe JSONL pozostaja lokalnym dowodem i nie sa przeznaczone do commita.

## Pytanie

Czy stala para `(anchor, guard)` moze ograniczyc ciecie przyszlego prawego ogona, uzywajac tylko no-lookahead early path oraz candidate-time fields?

## Zakres dowodowy

| scope | positions_with_exit_replay | positions_with_tsv2_windows | candidate_positions | exact_join_rate |
| --- | --- | --- | --- | --- |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | 4748 | 5604 | 5485 | 1.0 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | 3656 | 4831 | 4746 | 1.0 |

## Predeklarowane kotwice

- `M0_ALL / 6000 / -6000 / 120000`
- `M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000`
- `M7_CLASS_RESTRICTED / 10000 / -6000 / 60000`

## Predeklarowane guardy

- `G0_NONE`: brak dodatkowej ochrony.
- `G1_STEADY_EARLY_STRENGTH`: chroni tylko gdy path znany do `min(candidate_action_age, horizon)` ma `max >= 500 bps`, `last >= 250 bps`, `min >= -300 bps`.
- `G2_RECOVERY_AFTER_EARLY_DRAWDOWN`: chroni tylko gdy path ma drawdown `min <= -300 bps`, recovery `last - min >= 300 bps`, `last >= -100 bps`.
- `G3_LOW_VOL_CONTINUATION`: chroni tylko gdy path ma co najmniej 3 punkty, zakres `<= 350 bps`, `last >= 0 bps`, `max >= 100 bps`.
- `G4_DELAYED_DECISION_4000`: symuluje decyzje po 4000 ms, uzywajac tylko stanu dostepnego po opoznieniu.
- `G5_DELAYED_DECISION_8000`: symuluje decyzje po 8000 ms, uzywajac tylko stanu dostepnego po opoznieniu.

Horyzonty early path: `10000, 20000, 30000, 45000` ms.

## Wynik intersection

Passing fixed pairs across R49 and R50: `0`

Scope-pass rows: `0`

Best diagnostic row: `M0_ALL / 6000 / -6000 / 120000 + G2_RECOVERY_AFTER_EARLY_DRAWDOWN @ 30000ms on shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`

| anchor | guard_name | early_horizon_ms | r49_scope_pass | r50_scope_pass | fixed_pair_passing_both | r49_paired_delta_sum_bps | r50_paired_delta_sum_bps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| M0_ALL / 6000 / -6000 / 120000 | G0_NONE | 10000 | False | False | False | 426059 | 571588 |
| M0_ALL / 6000 / -6000 / 120000 | G0_NONE | 20000 | False | False | False | 426059 | 571588 |
| M0_ALL / 6000 / -6000 / 120000 | G0_NONE | 30000 | False | False | False | 426059 | 571588 |
| M0_ALL / 6000 / -6000 / 120000 | G0_NONE | 45000 | False | False | False | 426059 | 571588 |
| M0_ALL / 6000 / -6000 / 120000 | G1_STEADY_EARLY_STRENGTH | 10000 | False | False | False | 339326 | 507573 |
| M0_ALL / 6000 / -6000 / 120000 | G1_STEADY_EARLY_STRENGTH | 20000 | False | False | False | 347045 | 441529 |
| M0_ALL / 6000 / -6000 / 120000 | G1_STEADY_EARLY_STRENGTH | 30000 | False | False | False | 365277 | 464614 |
| M0_ALL / 6000 / -6000 / 120000 | G1_STEADY_EARLY_STRENGTH | 45000 | False | False | False | 354310 | 470308 |
| M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 10000 | False | False | False | 462473 | 589601 |
| M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 20000 | False | False | False | 458396 | 606076 |
| M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 30000 | False | False | False | 422175 | 618514 |
| M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 45000 | False | False | False | 428577 | 614998 |
| M0_ALL / 6000 / -6000 / 120000 | G3_LOW_VOL_CONTINUATION | 10000 | False | False | False | 399945 | 530332 |
| M0_ALL / 6000 / -6000 / 120000 | G3_LOW_VOL_CONTINUATION | 20000 | False | False | False | 379947 | 514040 |
| M0_ALL / 6000 / -6000 / 120000 | G3_LOW_VOL_CONTINUATION | 30000 | False | False | False | 378774 | 514040 |
| M0_ALL / 6000 / -6000 / 120000 | G3_LOW_VOL_CONTINUATION | 45000 | False | False | False | 378774 | 514040 |
| M0_ALL / 6000 / -6000 / 120000 | G4_DELAYED_DECISION_4000 | 10000 | False | False | False | 413612 | 472643 |
| M0_ALL / 6000 / -6000 / 120000 | G4_DELAYED_DECISION_4000 | 20000 | False | False | False | 413612 | 472643 |
| M0_ALL / 6000 / -6000 / 120000 | G4_DELAYED_DECISION_4000 | 30000 | False | False | False | 413612 | 472643 |
| M0_ALL / 6000 / -6000 / 120000 | G4_DELAYED_DECISION_4000 | 45000 | False | False | False | 413612 | 472643 |
| M0_ALL / 6000 / -6000 / 120000 | G5_DELAYED_DECISION_8000 | 10000 | False | False | False | 411949 | 410276 |
| M0_ALL / 6000 / -6000 / 120000 | G5_DELAYED_DECISION_8000 | 20000 | False | False | False | 411949 | 410276 |
| M0_ALL / 6000 / -6000 / 120000 | G5_DELAYED_DECISION_8000 | 30000 | False | False | False | 411949 | 410276 |
| M0_ALL / 6000 / -6000 / 120000 | G5_DELAYED_DECISION_8000 | 45000 | False | False | False | 411949 | 410276 |
| M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G0_NONE | 10000 | False | False | False | 442815 | 439644 |
| M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G0_NONE | 20000 | False | False | False | 442815 | 439644 |
| M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G0_NONE | 30000 | False | False | False | 442815 | 439644 |
| M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G0_NONE | 45000 | False | False | False | 442815 | 439644 |
| M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G1_STEADY_EARLY_STRENGTH | 10000 | False | False | False | 372147 | 376576 |
| M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G1_STEADY_EARLY_STRENGTH | 20000 | False | False | False | 382272 | 322851 |

_Pokazano 30 z 72 wierszy._

## Najlepsze wiersze diagnostyczne

| scope | anchor | guard_name | early_horizon_ms | scope_pass | exit_action_precision | wilson_lower95 | paired_delta_sum_bps | target_cut_damage_ratio | cost100_improvement_vs_unguarded_anchor_bps |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 30000 | False | 0.7692307692307693 | 0.7486276755761988 | 618514 | 0.35936488817393464 | 46926 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 45000 | False | 0.7698646262507357 | 0.7492544022573558 | 614998 | 0.3612096785969831 | 43410 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 20000 | False | 0.7741336633663366 | 0.7531100012036723 | 478572 | 0.2881101881092459 | 38928 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 10000 | False | 0.7350993377483444 | 0.7158742076596731 | 462473 | 0.3843700486843148 | 36414 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 20000 | False | 0.7671554252199414 | 0.7465070541374736 | 606076 | 0.36104839592504256 | 34488 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 10000 | False | 0.7398134511536574 | 0.7203219589631779 | 477221 | 0.20868474367971557 | 34406 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 20000 | False | 0.736441484300666 | 0.7171885511645543 | 458396 | 0.3889460050423756 | 32337 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M7_CLASS_RESTRICTED / 10000 / -6000 / 60000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 10000 | False | 0.6562318840579711 | 0.6334931888057067 | 96335 | 0.19282218625184663 | 28822 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 30000 | False | 0.7737135771853689 | 0.7526574244748744 | 467216 | 0.29633931309971495 | 27572 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M7_CLASS_RESTRICTED / 10000 / -6000 / 60000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 45000 | False | 0.6565950029052876 | 0.633834369856853 | 92565 | 0.20191448900664516 | 25052 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M7_CLASS_RESTRICTED / 10000 / -6000 / 60000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 20000 | False | 0.6569428238039673 | 0.6341397823884041 | 91881 | 0.20366720806651287 | 24368 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M7_CLASS_RESTRICTED / 10000 / -6000 / 60000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 30000 | False | 0.6569598136284217 | 0.634177194935016 | 89769 | 0.20271023963646098 | 22256 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 10000 | False | 0.7662416514875531 | 0.7451971309570697 | 460297 | 0.3073866116090912 | 20653 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 10000 | False | 0.7636786961583236 | 0.7430159195202191 | 589601 | 0.36022394335675134 | 18013 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 20000 | False | 0.7432296890672017 | 0.7236004440107096 | 458689 | 0.18833399033900286 | 15874 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M7_CLASS_RESTRICTED / 10000 / -6000 / 60000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 10000 | False | 0.7159722222222222 | 0.6921301326170985 | 512245 | 0.1144943996069102 | 13874 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M7_CLASS_RESTRICTED / 10000 / -6000 / 60000 | G1_STEADY_EARLY_STRENGTH | 10000 | False | 0.6703155183515775 | 0.6465399886035123 | 75038 | 0.16251828909216634 | 7525 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 45000 | False | 0.7732919254658385 | 0.7522031872544356 | 442601 | 0.31288960092148455 | 2957 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M0_ALL / 6000 / -6000 / 120000 | G2_RECOVERY_AFTER_EARLY_DRAWDOWN | 45000 | False | 0.7345679012345679 | 0.7152944584901344 | 428577 | 0.3975801510478225 | 2518 |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000 | G4_DELAYED_DECISION_4000 | 10000 | False | 0.7294058911632552 | 0.7095245327795011 | 443988 | 0.20296714653779532 | 1173 |

_Pokazano 20 z 144 wierszy._

## Decyzja

Runtime approval: `false`

Shadow_close_only approval: `false`

R51 GO/NO-GO: `NO_GO_FOR_R51`

No active close.

No Gatekeeper/BUY/REJECT/selector/TX/Jito/live change.

TSV2 remains diagnostic/logging-only.

ORG/TSV2/EIX/RTP provide no basis for runtime or `shadow_close_only`.

Jesli RTP-A0 nie ma stalej pary przechodzacej R49 i R50, kierunek pozostaje TSV2 diagnostic/logging-only. NO_GO_FOR_R51.

## Outputy

- `reports/selector/rtp_a0_guard_summary.csv`
- `reports/selector/rtp_a0_guard_stability.csv`
- `reports/selector/rtp_a0_tail_preservation.csv`
- `reports/selector/rtp_a0_fixed_pair_intersection.csv`
- `docs/ADR/ADR_8D_RTP_A0_RIGHT_TAIL_PRESERVATION_20260628.md`
