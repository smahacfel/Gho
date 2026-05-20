# RAPORT P3.7-J3O R15-r8i Smoke

Status: `NOT_READY_DIAGNOSED`

## Cel

Zweryfikować po J3O, czy configured shadow payer i wariantowo rozdzielony
kontrakt kont pozwalają counterfactual probe przejść do transport/entry bez
wymuszania routed-only kont na legacy candidates.

## Wynik

R15-r8i został zatrzymany wcześnie po pojawieniu się jednoznacznego blockera.
Nie czekano na pełny timeout, bo nie było sensu dalej zbierać tej samej klasy
skipów.

```text
namespace = shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8i
preflight = PASS
configured payer = wallets/shadow-burnin-test.json
probe_selection_rows = 11
probe_skips_rows = 13
probe_transport_rows = 0
probe_entry_rows = 0
active_buy_rows = 0
```

## Skip Breakdown

```text
verdict_type_not_in_sample_scope = 3
execution_account_not_ready = 10

execution_account_not_ready roles:
  bonding_curve_v2 = 8
  creator_vault = 2
```

All readiness skips used `probe_execution_account_wait_result=wait_timeout`.

## Interpretation

J3O confirmed that the payer path is no longer the immediate blocker: the
profile used the historical configured shadow-burnin payer path and startup
accepted it.

The remaining blocker shows that probe candidates still reached request
preparation as routed buys. That means the legacy route evidence was either not
selected or was lost before prepared-request construction. Follow-up inspection
identified the latter: trigger preparation sanitized `LegacyBuy` to `None`, and
the later build step defaulted `None` to `RoutedExactSolIn`.

## Decision

```text
R15-r8i smoke: NOT_READY_DIAGNOSED
J3O account-layout split: useful but insufficient
current blocker: LegacyBuy lost at prepared-request sanitization boundary
next repair: P3.7-J3P Probe Legacy Route Preservation Through Preparation
collection: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```

## Non-Goals Preserved

- no collection
- no P2/live
- no active policy change
- no IWIM change
- no threshold tuning
- no precheck bypass
- no treatment of missing execution accounts as success
