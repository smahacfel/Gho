#[test]
fn tx_intelligence_engine_avoids_account_state_core_paths() {
    let engine_src = include_str!("../src/tx_intelligence/engine.rs");

    assert!(
        !engine_src.contains("account_state_core"),
        "tx_intelligence/engine.rs must not import account_state_core"
    );
}

#[test]
fn gatekeeper_policy_stays_feature_only() {
    let policy_src = include_str!("../src/components/gatekeeper_policy.rs");

    assert!(
        !policy_src.contains("PoolTransaction"),
        "gatekeeper_policy.rs must not depend on raw PoolTransaction"
    );
    assert!(
        !policy_src.contains("shadow_ledger::"),
        "gatekeeper_policy.rs should avoid direct shadow_ledger module paths"
    );
    assert!(
        policy_src.contains("early_fingerprint: None"),
        "build_assessment_from_features must start with early_fingerprint cleared so policy input stays feature-only"
    );
}

#[test]
fn oracle_runtime_keeps_early_fingerprint_post_verdict_only() {
    let runtime_src = include_str!("../src/oracle_runtime.rs");
    let attachments: Vec<usize> = runtime_src
        .match_indices("assessment.early_fingerprint = Some")
        .map(|(idx, _)| idx)
        .collect();

    assert_eq!(
        attachments.len(),
        3,
        "oracle_runtime.rs should only attach early_fingerprint in the three terminal verdict arms"
    );
    assert!(
        !runtime_src.contains("refresh_assessment_thresholds("),
        "oracle_runtime.rs must not re-score policy with early_fingerprint before terminal verdict"
    );

    let reject_arm = runtime_src
        .find(
            "GatekeeperVerdict::Reject {\n                mut assessment,\n                reason,\n            } => {",
        )
        .expect("reject terminal arm should exist");
    let timeout_arm = runtime_src
        .find("GatekeeperVerdict::Timeout { mut assessment } => {")
        .expect("timeout terminal arm should exist");
    let buy_arm = runtime_src
        .find(
            "GatekeeperVerdict::Buy {\n                buffered_txs,\n                mut assessment,\n            } => {",
        )
        .expect("buy terminal arm should exist");

    assert!(
        reject_arm < attachments[0] && attachments[0] < timeout_arm,
        "reject path should attach early_fingerprint only after entering the reject terminal arm"
    );
    assert!(
        timeout_arm < attachments[1] && attachments[1] < buy_arm,
        "timeout path should attach early_fingerprint only after entering the timeout terminal arm"
    );
    assert!(
        buy_arm < attachments[2],
        "buy path should attach early_fingerprint only after entering the buy terminal arm"
    );
}

#[test]
fn oracle_runtime_no_longer_has_per_pool_session_bridge() {
    let runtime_src = include_str!("../src/oracle_runtime.rs");

    assert!(
        !runtime_src.contains("session: Option<SharedSession>"),
        "PerPoolOracleState should no longer cache SharedSession handles"
    );
    assert!(
        !runtime_src.contains("fn bind_session_to_pool"),
        "OracleRuntime should not maintain a PerPoolOracleState -> Session bridge"
    );
}

#[test]
fn phase6_production_code_omits_legacy_account_update_switch_name() {
    let legacy_switch_name = ["account", "updates", "enabled"].join("_");
    let launcher_config_src = include_str!("../src/config.rs");
    let launcher_runtime_src = include_str!("../src/oracle_runtime.rs");
    let launcher_seer_src = include_str!("../src/components/seer.rs");
    let seer_config_src = include_str!("../../off-chain/components/seer/src/config.rs");

    assert!(
        !launcher_config_src.contains(&legacy_switch_name),
        "launcher config must not expose the legacy account update switch name"
    );
    assert!(
        !launcher_runtime_src.contains(&legacy_switch_name),
        "oracle runtime must not branch on the legacy account update switch name"
    );
    assert!(
        !launcher_seer_src.contains(&legacy_switch_name),
        "launcher seer bridge must not expose the legacy account update switch name"
    );
    assert!(
        !seer_config_src.contains(&legacy_switch_name),
        "seer config must not expose the legacy account update switch name"
    );
}

#[test]
fn oracle_runtime_omits_legacy_feature_verdict_helper_name() {
    let runtime_src = include_str!("../src/oracle_runtime.rs");
    let gatekeeper_src = include_str!("../src/components/gatekeeper.rs");

    assert!(
        !runtime_src.contains("evaluate_from_features_legacy("),
        "oracle_runtime.rs must not call the legacy feature verdict helper"
    );
    assert!(
        !gatekeeper_src.contains("evaluate_from_features_legacy("),
        "gatekeeper.rs must not expose the legacy feature verdict helper name"
    );
}

#[test]
fn post_buy_runtime_uses_canonical_live_price_contract() {
    let post_buy_src = include_str!("../src/components/post_buy_runtime.rs");
    let implementation = post_buy_src
        .split("#[cfg(test)]")
        .next()
        .expect("implementation section should exist");

    assert!(
        implementation.contains("account_state_core"),
        "post_buy_runtime.rs must depend on AccountStateCore for live pricing"
    );
    assert!(
        implementation.contains("canonical_account_state"),
        "post_buy_runtime.rs must meter canonical AccountStateCore price hits"
    );
    assert!(
        implementation.contains("rpc_point_query"),
        "post_buy_runtime.rs must meter point-query fallback hits"
    );
    assert!(
        !implementation.contains("fn read_price_from_shadow("),
        "Phase 4 must remove the live shadow price helper from post-buy runtime"
    );
    assert!(
        !implementation.contains("\"source\" => \"shadow_ledger\""),
        "post_buy live pricing must not advertise ShadowLedger as truth source"
    );

    let main_src = include_str!("../src/main.rs");
    assert!(
        main_src.contains("Arc::clone(oracle_runtime.account_state_core())"),
        "launcher startup must wire AccountStateCore into LiveSellHandle"
    );
}

#[test]
fn post_buy_runtime_stage1_live_exit_lane_is_sender_only() {
    let post_buy_src = include_str!("../src/components/post_buy_runtime.rs");
    let implementation = post_buy_src
        .split("#[cfg(test)]")
        .next()
        .expect("implementation section should exist");

    assert!(
        implementation.contains("LiveExitSession::new("),
        "live post-buy path must create a dedicated stage-1 exit session"
    );
    assert!(
        implementation.contains("EntryPriceExtractor::new"),
        "stage-1 live exit must extract confirmed entry price from BUY transaction metadata"
    );
    assert!(
        implementation.contains("extract_with_retry("),
        "stage-1 live exit must retry confirmed BUY metadata extraction"
    );
    assert!(
        implementation.contains("build_full_exit_transaction_with_retry("),
        "stage-1 live exit must build a fresh Sender SELL transaction"
    );
    assert!(
        implementation.contains("submit_live_exit_transaction("),
        "stage-1 live exit must submit the SELL through the Sender-owned runtime path"
    );
    assert!(
        implementation.contains(".send_transaction(&transaction)"),
        "stage-1 live exit must use Helius Sender transport for SELL submission"
    );
    assert!(
        implementation.contains("confirm_sender_sell_attempt("),
        "stage-1 live exit must confirm the submitted SELL through the unified Sender SELL confirmation helper"
    );
    assert!(
        implementation.contains(".confirm_submission_with_timeout(submission, max_wait_ms)"),
        "Sender SELL confirmation helper must still call live_tx_sender confirmation under timeout control"
    );
    assert!(
        !implementation.contains("create_tip_transaction("),
        "stage-1 live exit must not append a dedicated legacy Jito tip transaction"
    );
    assert!(
        !implementation.contains("confirm_bundle_submission_with_balance("),
        "stage-1 live exit must not depend on legacy Jito bundle confirmation"
    );
    assert!(
        !implementation.contains("submit_bundle_with_redundancy_receipt("),
        "stage-1 live exit must not submit SELL via legacy Jito bundle transport"
    );
    assert!(
        !implementation.contains("run_live_sell_lifecycle_legacy("),
        "stage-1 live exit must not keep a dormant legacy SELL lifecycle in production code"
    );
    assert!(
        !implementation.contains("JitoClient"),
        "stage-1 live exit implementation must not construct or depend on a Jito client"
    );
    assert!(
        implementation.contains("ExitTriggeredTakeProfit")
            && implementation.contains("ExitTriggeredStopLoss"),
        "stage-1 live exit must expose explicit TP and SL trigger states"
    );
}

#[test]
fn trigger_component_live_buy_stays_fail_closed_on_sender() {
    let trigger_src = include_str!("../src/components/trigger/component.rs");

    assert!(
        trigger_src.contains("ensure_live_sender_transport"),
        "live BUY trigger must keep an explicit fail-closed Sender transport guard"
    );
    assert!(
        trigger_src.contains("RPC fallback is disabled"),
        "live BUY guard must refuse RPC/standard transaction fallback"
    );
    assert!(
        trigger_src.contains("Helius Sender + Yellowstone transport"),
        "live BUY guard must require initialized Sender transport"
    );
    assert!(
        trigger_src.contains("submit_prepared_via_sender("),
        "live BUY dispatch must route through the Sender submit path"
    );
    assert!(
        trigger_src.contains(".send_transaction(&current_request.buy_tx)"),
        "live BUY dispatch must submit the prepared BUY through Helius Sender"
    );
    assert!(
        trigger_src.contains(".confirm_sender_buy_attempt("),
        "live BUY dispatch must explicitly confirm Sender submission"
    );
    assert!(
        !trigger_src.contains("submit_bundle_and_confirm_with_balance("),
        "live BUY dispatch must not submit legacy Jito bundles"
    );
}

#[test]
fn post_buy_runtime_live_lane_stays_sender_only() {
    let post_buy_src = include_str!("../src/components/post_buy_runtime.rs");
    let implementation = post_buy_src
        .split("#[cfg(test)]")
        .next()
        .expect("implementation section should exist");

    assert_eq!(
        implementation.matches("run_live_sell_lifecycle(").count(),
        2,
        "live sell runtime should keep a single authoritative lifecycle definition and spawn path"
    );
    assert!(
        !implementation.contains("run_live_sell_lifecycle_legacy("),
        "live post-buy lane must not keep a legacy SELL lifecycle helper in production code"
    );
    assert!(
        implementation.contains("run_live_sell_lifecycle(")
            && implementation.contains("submit_live_exit_transaction("),
        "live post-buy lane must keep using the authoritative Sender sell lifecycle"
    );
}
