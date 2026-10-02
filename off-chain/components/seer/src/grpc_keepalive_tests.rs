//! Regresje rzeczywistej pętli klienta na lokalnym serwerze Yellowstone.
use super::*;
use yellowstone_grpc_proto::geyser::geyser_server::{Geyser, GeyserServer};
use yellowstone_grpc_proto::prelude::*;

struct PingChallengeServer {
    result: Arc<Mutex<Option<tokio::sync::oneshot::Sender<bool>>>>,
}

#[tonic::async_trait]
impl Geyser for PingChallengeServer {
    type SubscribeStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = std::result::Result<SubscribeUpdate, Status>> + Send>,
    >;

    async fn subscribe(
        &self,
        request: Request<tonic::Streaming<SubscribeRequest>>,
    ) -> std::result::Result<tonic::Response<Self::SubscribeStream>, Status> {
        let mut requests = request.into_inner();
        let result = self.result.lock().take().unwrap();
        let stream = async_stream::stream! {
            // Odbierz subskrypcję i pierwszy ping timera. Kolejny termin to 10 s.
            let first = requests.message().await.unwrap().unwrap();
            assert!(first.ping.is_none());
            let ping = tokio::time::timeout(Duration::from_secs(2), requests.message()).await.unwrap().unwrap().unwrap();
            let mut last_id = ping.ping.unwrap().id;
            yield Ok(SubscribeUpdate { filters: vec![], update_oneof: Some(UpdateOneof::Pong(SubscribeUpdatePong { id: last_id })) });
            // Dwa pytania serwera wymagają dwóch odpowiedzi, bez zmiany filtrów.
            let mut passed = true;
            for _ in 0..2 {
                tokio::time::sleep(Duration::from_millis(30)).await;
                yield Ok(SubscribeUpdate { filters: vec![], update_oneof: Some(UpdateOneof::Ping(SubscribeUpdatePing {})) });
                let reply = tokio::time::timeout(Duration::from_millis(750), requests.message()).await;
                match reply {
                    Ok(Ok(Some(req))) => {
                        passed &= req.accounts.is_empty() && req.transactions.is_empty() && req.slots.is_empty() && req.commitment.is_none();
                        if let Some(p) = req.ping {
                            passed &= p.id > last_id;
                            last_id = p.id;
                            yield Ok(SubscribeUpdate { filters: vec![], update_oneof: Some(UpdateOneof::Pong(SubscribeUpdatePong { id: p.id })) });
                        } else { passed = false; break; }
                    }
                    _ => { passed = false; break; }
                }
            }
            let _ = result.send(passed);
            // Utrzymaj połączenie do jawnego anulowania przez test.
            futures::future::pending::<()>().await;
        };
        Ok(tonic::Response::new(Box::pin(stream)))
    }
    async fn ping(
        &self,
        _: Request<PingRequest>,
    ) -> std::result::Result<tonic::Response<PongResponse>, Status> {
        Err(Status::unimplemented("test"))
    }
    async fn get_latest_blockhash(
        &self,
        _: Request<GetLatestBlockhashRequest>,
    ) -> std::result::Result<tonic::Response<GetLatestBlockhashResponse>, Status> {
        Err(Status::unimplemented("test"))
    }
    async fn get_block_height(
        &self,
        _: Request<GetBlockHeightRequest>,
    ) -> std::result::Result<tonic::Response<GetBlockHeightResponse>, Status> {
        Err(Status::unimplemented("test"))
    }
    async fn get_slot(
        &self,
        _: Request<GetSlotRequest>,
    ) -> std::result::Result<tonic::Response<GetSlotResponse>, Status> {
        Err(Status::unimplemented("test"))
    }
    async fn is_blockhash_valid(
        &self,
        _: Request<IsBlockhashValidRequest>,
    ) -> std::result::Result<tonic::Response<IsBlockhashValidResponse>, Status> {
        Err(Status::unimplemented("test"))
    }
    async fn get_version(
        &self,
        _: Request<GetVersionRequest>,
    ) -> std::result::Result<tonic::Response<GetVersionResponse>, Status> {
        Err(Status::unimplemented("test"))
    }
}

#[tokio::test]
async fn server_ping_gets_prompt_response_on_primary_and_funding() {
    for profile in [
        GrpcSubscriptionProfile::Gate0Observation,
        GrpcSubscriptionProfile::FundingLaneFullChain,
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let stop = CancellationToken::new();
        let server_stop = stop.clone();
        let server = tokio::spawn(async move {
            let incoming = async_stream::stream! {
                loop { yield listener.accept().await.map(|(socket, _)| socket); }
            };
            tonic::transport::Server::builder()
                .add_service(GeyserServer::new(PingChallengeServer {
                    result: Arc::new(Mutex::new(Some(result_tx))),
                }))
                .serve_with_incoming_shutdown(incoming, server_stop.cancelled())
                .await
                .unwrap();
        });
        let config = GrpcConfig {
            providers: vec![Provider::new(endpoint, None, "local-test")],
            subscription_profile: profile,
            ..Default::default()
        };
        let (connector, rx, _gaps) = YellowstoneConnector::new(config);
        let shutdown = connector.shutdown_handle();
        let token = connector.shutdown_token_handle();
        let stats = connector.stats();
        let client = tokio::spawn(connector.run());
        let result = tokio::time::timeout(Duration::from_secs(4), result_rx).await;
        shutdown.store(true, Ordering::Release);
        token.cancel();
        tokio::time::timeout(Duration::from_secs(2), client)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
        assert!(
            result.unwrap().unwrap(),
            "Klient zignorował Ping serwera: {profile:?}"
        );
        assert_eq!(stats.pings_sent.load(Ordering::Relaxed), 3);
        // Ping/Pong nie mogą trafić do kolejki danych ani udawać postępu rynku.
        while let Ok(event) = rx.queue.try_recv() {
            assert!(matches!(event, PumpEvent::PrimaryTradeFeedProgress { .. }));
        }
    }
}

#[tokio::test]
async fn gate0_registry_changes_never_replace_source_subscription() {
    let registry = AccountRegistry::new();
    let profile = GrpcSubscriptionProfile::Gate0Observation;
    let original =
        build_subscribe_request_for_profile(CommitmentLevel::Confirmed, profile, &registry, 0);
    let mut version = registry.version();
    let mut fingerprint = subscribe_request_fingerprint_for_profile(profile, &registry);
    let cfg = GrpcConfig {
        subscription_profile: profile,
        commitment: CommitmentLevel::Confirmed,
        ..Default::default()
    };
    let slots = SlotTracker::new();
    let stats = Arc::new(TransportStats::default());
    let (mut sink, mut receiver) = futures::channel::mpsc::unbounded::<SubscribeRequest>();
    let mut last_resub = Instant::now() - Duration::from_secs(10);
    registry.insert_curve("local-curve");
    registry.insert_pool("local-pool");
    registry.insert_mint("local-mint");
    registry.insert("unused-execution-account");
    registry.insert_bcv2("unused-execution-vault");
    for reason in [
        "health_tick",
        "registry_notify",
        "bcv2_registry_notify",
        "registry_tick",
    ] {
        maybe_send_resubscribe(
            "test",
            reason,
            &mut sink,
            &cfg,
            &registry,
            &slots,
            &stats,
            None,
            &mut version,
            &mut fingerprint,
            &mut last_resub,
            true,
            true,
        )
        .await
        .unwrap();
    }
    assert!(
        receiver.try_recv().is_err(),
        "Gate0 nie może zmieniać globalnego źródła po rejestracji lokalnego konta"
    );
    assert_eq!(stats.resubs_sent.load(Ordering::Relaxed), 0);
    let current =
        build_subscribe_request_for_profile(CommitmentLevel::Confirmed, profile, &registry, 99);
    assert_eq!(current, original);
    assert_eq!(
        current.accounts["pumpfun_curve_layouts"].owner,
        vec![PUMP_FUN_PROGRAM_ID]
    );
    assert_eq!(
        current.accounts["pumpswap_pool_layouts"].owner,
        vec![PUMP_SWAP_PROGRAM_ID]
    );
    assert_eq!(
        current.accounts["tracked_accounts"].account,
        vec![PUMP_FUN_FEE_ACCOUNT]
    );
    assert_eq!(current.transactions["pump_txs"].failed, None);
    assert_eq!(current.commitment, Some(CommitmentLevel::Confirmed as i32));
    assert!(!current.blocks_meta.is_empty());
    // Zwykły runtime nadal ma kontrakt dynamicznych kont wykonania.
    let ordinary = build_subscribe_request_for_profile(
        CommitmentLevel::Confirmed,
        GrpcSubscriptionProfile::PrimaryGlobal,
        &registry,
        99,
    );
    assert!(ordinary.accounts["tracked_accounts"]
        .account
        .contains(&"unused-execution-vault".to_owned()));
}
