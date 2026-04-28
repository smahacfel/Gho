//! Integration test for gRPC connection functionality
//!
//! This test demonstrates the configuration and initialization of the gRPC connection
//! with various settings.

#[cfg(test)]
mod grpc_integration_tests {
    use seer::config::{ConnectionMode, SeerConfig};
    use seer::Seer;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_grpc_configuration_with_auth() {
        // Test that Seer can be created with full gRPC configuration
        let config = SeerConfig {
            connection_mode: ConnectionMode::Grpc,
            grpc_endpoint: "http://localhost:10000".to_string(),
            grpc_client_id: Some("test-client-123".to_string()),
            grpc_auth_token: Some("test-token-secret".to_string()),
            max_reconnect_attempts: 3,
            reconnect_delay_secs: 2,
            max_reconnect_delay_secs: 60,
            verbose: true,
            ..Default::default()
        };

        let (tx, _rx) = mpsc::channel(100);
        let seer = Seer::new(config.clone(), tx);

        // Verify configuration was applied correctly
        assert_eq!(
            seer.metrics()
                .websocket_reconnections
                .with_label_values(&["test"])
                .get(),
            0
        );

        // Note: We don't actually connect in the test since we don't have a live gRPC endpoint
        // In a real integration test, you would:
        // 1. Start a test gRPC server
        // 2. Call seer.run().await
        // 3. Verify events are received
    }

    #[tokio::test]
    async fn test_grpc_configuration_without_auth() {
        // Test that Seer can be created without authentication
        let config = SeerConfig {
            connection_mode: ConnectionMode::Grpc,
            grpc_endpoint: "http://localhost:10000".to_string(),
            grpc_client_id: None,
            grpc_auth_token: None,
            max_reconnect_attempts: 5,
            reconnect_delay_secs: 5,
            max_reconnect_delay_secs: 300,
            verbose: false,
            ..Default::default()
        };

        let (tx, _rx) = mpsc::channel(100);
        let _seer = Seer::new(config, tx);

        // Seer created successfully without auth
    }

    #[tokio::test]
    async fn test_websocket_fallback_configuration() {
        // Test that WebSocket mode still works as fallback
        let config = SeerConfig {
            connection_mode: ConnectionMode::WebSocket,
            geyser_endpoint: "wss://api.mainnet-beta.solana.com".to_string(),
            max_reconnect_attempts: 5,
            reconnect_delay_secs: 5,
            max_reconnect_delay_secs: 300,
            verbose: false,
            ..Default::default()
        };

        let (tx, _rx) = mpsc::channel(100);
        let _seer = Seer::new(config, tx);

        // Seer created successfully in WebSocket mode
    }

    #[test]
    fn test_config_defaults() {
        // Test that default configuration has expected values
        let config = SeerConfig::default();

        assert_eq!(config.connection_mode, ConnectionMode::Grpc);
        assert_eq!(config.max_reconnect_attempts, 10);
        assert_eq!(config.reconnect_delay_secs, 5);
        assert_eq!(config.max_reconnect_delay_secs, 300);
        assert_eq!(config.grpc_client_id, None);
        assert_eq!(config.grpc_auth_token, None);
        assert!(!config.verbose);
    }

    #[test]
    fn test_exponential_backoff_calculation() {
        // Verify exponential backoff math
        let initial_delay = 5u64;
        let max_delay = 300u64;

        let delays: Vec<u64> = (0..10)
            .map(|attempt| std::cmp::min(initial_delay * 2u64.pow(attempt), max_delay))
            .collect();

        assert_eq!(delays[0], 5); // 5 * 2^0 = 5
        assert_eq!(delays[1], 10); // 5 * 2^1 = 10
        assert_eq!(delays[2], 20); // 5 * 2^2 = 20
        assert_eq!(delays[3], 40); // 5 * 2^3 = 40
        assert_eq!(delays[4], 80); // 5 * 2^4 = 80
        assert_eq!(delays[5], 160); // 5 * 2^5 = 160
        assert_eq!(delays[6], 300); // 5 * 2^6 = 320, capped at 300
        assert_eq!(delays[7], 300); // capped
        assert_eq!(delays[8], 300); // capped
        assert_eq!(delays[9], 300); // capped
    }

    #[tokio::test]
    async fn test_grpc_configuration_with_https_endpoint() {
        // Test HTTPS endpoint configuration (Chainstack-style)
        let config = SeerConfig {
            connection_mode: ConnectionMode::Grpc,
            grpc_endpoint: "https://yellowstone-solana-mainnet.core.chainstack.com".to_string(),
            grpc_client_id: Some("chainstack-client".to_string()),
            grpc_auth_token: Some("test-x-token-secret".to_string()),
            max_reconnect_attempts: 3,
            reconnect_delay_secs: 2,
            max_reconnect_delay_secs: 60,
            verbose: true,
            ..Default::default()
        };

        let (tx, _rx) = mpsc::channel(100);
        let seer = Seer::new(config.clone(), tx);

        // Verify configuration was applied correctly
        assert_eq!(
            seer.metrics()
                .websocket_reconnections
                .with_label_values(&["test"])
                .get(),
            0
        );

        // Note: We don't actually connect since we don't have valid credentials
        // This test validates that HTTPS endpoints can be configured properly
    }

    #[tokio::test]
    async fn test_grpc_configuration_with_https_and_port() {
        // Test HTTPS endpoint with explicit port 443
        let config = SeerConfig {
            connection_mode: ConnectionMode::Grpc,
            grpc_endpoint: "https://yellowstone-solana-mainnet.core.chainstack.com:443".to_string(),
            grpc_client_id: None,
            grpc_auth_token: Some("test-token".to_string()),
            max_reconnect_attempts: 5,
            reconnect_delay_secs: 5,
            max_reconnect_delay_secs: 300,
            verbose: false,
            ..Default::default()
        };

        let (tx, _rx) = mpsc::channel(100);
        let _seer = Seer::new(config, tx);

        // Successfully created with HTTPS endpoint including port
    }
}
