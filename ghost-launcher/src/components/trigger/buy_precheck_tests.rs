// Testy kosztu RPC i zachowania fail-closed zbiorczego prechecku BUY.
async fn spawn_buy_precheck_rpc(
    missing: Pubkey,
    fault: Option<&'static str>,
) -> (String, Arc<std::sync::Mutex<Vec<(String, usize)>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&calls);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let mut buffer = Vec::new();
            let request = loop {
                let mut chunk = [0u8; 8192];
                let n = stream.read(&mut chunk).await.unwrap();
                if n == 0 {
                    return;
                }
                buffer.extend_from_slice(&chunk[..n]);
                if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                    if let Ok(value) =
                        serde_json::from_slice::<serde_json::Value>(&buffer[end + 4..])
                    {
                        break value;
                    }
                }
            };
            let method = request["method"].as_str().unwrap();
            let keys = if method == "getMultipleAccounts" {
                request["params"][0].as_array().unwrap().clone()
            } else {
                vec![request["params"][0].clone()]
            };
            if method == "getAccountInfo" || method == "getMultipleAccounts" {
                assert_eq!(request["params"][1]["commitment"], "processed");
                recorded
                    .lock()
                    .unwrap()
                    .push((method.to_string(), keys.len()));
            }
            let first_read_missing = fault == Some("first_read_missing")
                && method == "getAccountInfo"
                && recorded.lock().unwrap().len() == 1;
            let account = |key: &serde_json::Value| {
                if first_read_missing || key.as_str() == Some(missing.to_string().as_str()) {
                    serde_json::Value::Null
                } else {
                    serde_json::json!({"lamports":1,"owner":system_program::id().to_string(),
                        "data":["AQID","base64"],"executable":false,"rentEpoch":0})
                }
            };
            let result = match method {
                "getVersion" => serde_json::json!({"solana-core":"1.18.26","feature-set":1}),
                "getAccountInfo" => {
                    serde_json::json!({"context":{"slot":123},"value":account(&keys[0])})
                }
                "getMultipleAccounts" => {
                    let mut values: Vec<_> = keys.iter().map(account).collect();
                    if fault == Some("length") {
                        values.pop();
                    }
                    serde_json::json!({"context":{"slot":123},"value":values})
                }
                other => panic!("nieoczekiwane RPC: {other}"),
            };
            let body = if fault == Some("rpc") && method != "getVersion" {
                serde_json::json!({"jsonrpc":"2.0","id":request["id"],
                    "error":{"code":-32602,"message":"invalid batch request"}})
                .to_string()
            } else {
                serde_json::json!({"jsonrpc":"2.0","id":request["id"],"result":result}).to_string()
            };
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            stream.write_all(response.as_bytes()).await.unwrap();
            let _ = stream.shutdown().await;
        }
    });
    (format!("http://{addr}"), calls)
}

#[tokio::test]
async fn buy_precheck_batches_preserving_order_missing_role_and_account_evidence() {
    let present = Pubkey::new_unique();
    let missing = Pubkey::new_unique();
    let (url, calls) = spawn_buy_precheck_rpc(missing, None).await;
    let trigger = TriggerComponent::new(create_test_config_with_rpc_url(url));
    let checks = trigger
        .counterfactual_probe_manifest_account_checks(&[
            (present, "mint".into()),
            (missing, "curve".into()),
            (present, "duplicate".into()),
        ])
        .await
        .unwrap();
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[0].pubkey, present);
    assert_eq!(checks[0].role, "mint");
    assert!(checks[0].rpc_load_ready);
    assert_eq!(
        checks[0].account_owner,
        Some(system_program::id().to_string())
    );
    assert_eq!(checks[0].account_data_len, Some(3));
    assert_eq!(checks[1].pubkey, missing);
    assert_eq!(checks[1].role, "curve");
    assert!(!checks[1].rpc_load_ready);
    assert_eq!(
        checks[1].rpc_error_class.as_deref(),
        Some("account_missing")
    );
    assert!(checks
        .iter()
        .all(|c| c.context_slot == Some(123) && c.attempt_count == 1));
    assert_eq!(
        *calls.lock().unwrap(),
        vec![("getMultipleAccounts".into(), 2)]
    );
}

#[tokio::test]
async fn buy_precheck_fails_closed_on_rpc_error_or_incomplete_batch() {
    for fault in ["rpc", "length"] {
        let (url, _) = spawn_buy_precheck_rpc(Pubkey::new_unique(), Some(fault)).await;
        let trigger = TriggerComponent::new(create_test_config_with_rpc_url(url));
        let result = trigger
            .counterfactual_probe_manifest_account_checks(&[(Pubkey::new_unique(), "mint".into())])
            .await;
        assert!(result.is_err(), "brak blokady dla {fault}");
    }
}

#[tokio::test]
async fn buy_precheck_bounds_batches_and_skips_empty_input() {
    let (url, calls) = spawn_buy_precheck_rpc(Pubkey::new_unique(), None).await;
    let trigger = TriggerComponent::new(create_test_config_with_rpc_url(url));
    assert!(trigger
        .counterfactual_probe_manifest_account_checks(&[])
        .await
        .unwrap()
        .is_empty());
    assert!(calls.lock().unwrap().is_empty());
    let accounts: Vec<_> = (0..101)
        .map(|i| (Pubkey::new_unique(), format!("account_{i}")))
        .collect();
    let checks = trigger
        .counterfactual_probe_manifest_account_checks(&accounts)
        .await
        .unwrap();
    assert_eq!(checks.len(), accounts.len());
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            ("getMultipleAccounts".into(), 100),
            ("getMultipleAccounts".into(), 1)
        ]
    );
}

struct DeadlineTestSimulator {
    calls: Arc<AtomicUsize>,
    delay: Duration,
    report_lag_ms: u64,
}

#[async_trait]
impl ShadowSimulator for DeadlineTestSimulator {
    async fn simulate_buy(
        &self,
        request: &PreparedBuyRequest,
        config: &crate::config::TriggerShadowRunConfig,
    ) -> Result<super::super::shadow_run::ShadowBuySimulationReport> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        let mut report = MockShadowSimulator.simulate_buy(request, config).await?;
        report.simulation_finished_ts_ms = request.decision_ts_ms + self.report_lag_ms;
        Ok(report)
    }
}

#[tokio::test]
async fn buy_deadline_covers_preparation_rpc_and_late_result_without_leaking_position_slot() {
    for (budget, age, delay, reported_lag, expected_calls, expected_success) in [
        (Some(50), 100, 0, 5, 0, false),
        (Some(50), 0, 150, 5, 1, false),
        (Some(50), 0, 0, 51, 1, false),
        (Some(50), 0, 0, 5, 1, true),
        (None, 100, 0, 600, 1, true),
    ] {
        let mut config = create_test_config();
        config.entry_mode = TriggerEntryMode::LiveAndShadow;
        config.shadow_run.enabled = true;
        config.shadow_run.decision_to_buy_deadline_ms = budget;
        let calls = Arc::new(AtomicUsize::new(0));
        let trigger = TriggerComponent::new_with_shadow_simulator(
            config,
            Arc::new(DeadlineTestSimulator {
                calls: Arc::clone(&calls),
                delay: Duration::from_millis(delay),
                report_lag_ms: reported_lag,
            }),
        );
        let mut request = build_test_prepared_buy_request(
            &trigger,
            &Keypair::new(),
            &Pubkey::new_unique(),
            &Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap(),
            true,
            Some(0),
        );
        request.decision_ts_ms = TriggerComponent::now_ms().saturating_sub(age);
        let receipt = trigger.dispatch_prepared_buy_shadow_only(request).await;
        assert_eq!(receipt.primary_outcome.is_ok(), expected_success);
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        if let Err(err) = &receipt.primary_outcome {
            assert_eq!(
                super::super::shadow_run::classify_shadow_error(&err.to_string()),
                "execution_deadline_exceeded"
            );
        }
        drop(receipt);
        assert_eq!(trigger.active_positions(), 0);
    }
}

#[test]
fn buy_deadline_old_config_retains_unbounded_entry_budget() {
    let config: crate::config::TriggerShadowRunConfig = toml::from_str("enabled = true").unwrap();
    assert_eq!(config.decision_to_buy_deadline_ms, None);
}

#[tokio::test]
async fn buy_precheck_parallel_preparation_refreshes_blockhash_that_aged_during_account_fetch() {
    let payer = Keypair::new();
    let mint = Pubkey::new_unique();
    let legacy = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let token2022 = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap();
    let (url, _) = spawn_parallel_prepare_buy_rpc_server(
        payer.pubkey(),
        mint,
        get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &legacy),
        get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token2022),
        legacy,
        BUY_BLOCKHASH_CACHE_MAX_AGE_MS + 100,
        false,
        false,
    )
    .await;
    let temp = tempfile::tempdir().unwrap();
    let key = temp.path().join("payer.json");
    solana_sdk::signature::write_keypair_file(&payer, &key).unwrap();
    let mut config = create_test_config_with_rpc_url(url);
    config.keypair_path = Some(key.to_string_lossy().into_owned());
    let trigger = TriggerComponent::new(config);
    let request = trigger
        .prepare_buy_request(&mint, &valid_buy_account_overrides(), 1_000_000)
        .await
        .unwrap();
    assert!(request.blockhash_age_ms <= BUY_BLOCKHASH_CACHE_MAX_AGE_MS);
}

#[tokio::test]
async fn buy_precheck_retries_newly_visible_mint_without_150ms_idle_wait() {
    let (url, calls) =
        spawn_buy_precheck_rpc(Pubkey::new_unique(), Some("first_read_missing")).await;
    let trigger = TriggerComponent::new(create_test_config_with_rpc_url(url));
    let mint = Pubkey::new_unique();
    let account = tokio::time::timeout(
        Duration::from_millis(100),
        trigger.fetch_mint_account_with_retry(&mint, trigger.preparation_rpc()),
    )
    .await
    .expect("mint dostępny po pierwszym braku nie powinien czekać 150ms")
    .expect("drugi odczyt zwraca konto");
    assert_eq!(account.data, vec![1, 2, 3]);
    assert_eq!(
        *calls.lock().unwrap(),
        vec![("getAccountInfo".into(), 1), ("getAccountInfo".into(), 1)]
    );
}
