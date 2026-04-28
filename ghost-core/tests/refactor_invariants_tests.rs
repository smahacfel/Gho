#[test]
fn account_state_core_avoids_direct_shadow_ledger_paths() {
    let types_src = include_str!("../src/account_state_core/types.rs");
    let reducer_src = include_str!("../src/account_state_core/reducer.rs");

    assert!(
        !types_src.contains("shadow_ledger::"),
        "account_state_core/types.rs must not directly import shadow_ledger paths"
    );
    assert!(
        !reducer_src.contains("shadow_ledger::"),
        "account_state_core/reducer.rs must not directly import shadow_ledger paths"
    );
}

#[test]
fn reconciliation_runtime_stays_monitoring_only() {
    let source = include_str!("../src/shadow_ledger/reconciliation_runtime.rs");
    let implementation = source
        .split("#[cfg(test)]")
        .next()
        .expect("implementation section should exist");

    assert!(
        !implementation.contains(".record_diagnostic_signal("),
        "ReconciliationRuntime must not propagate diagnostic signals into hot-pool tracking"
    );
    assert!(
        !implementation.contains("shadow_ledger_runtime_diagnostic_signals_total"),
        "ReconciliationRuntime must not emit diagnostic metrics in monitoring-only mode"
    );
    assert!(
        implementation.contains("shadow_ledger_runtime_unexpected_reconciliation_action_total"),
        "ReconciliationRuntime must surface any unexpected legacy write-like action as compatibility drift"
    );
}
