# TPU Connection Manager Implementation

## Overview

This implementation provides QUIC-based connection management for Solana TPU (Transaction Processing Unit) with pre-warming capabilities, enabling optimal transaction submission latency through the "Leapfrog" optimization strategy.

## Components

### 1. LeaderResolver (`src/leader_resolver.rs`)

Resolves Solana validator leaders to their TPU contact information by querying cluster nodes and caching results.

#### Features
- Fetches cluster nodes via RPC `get_cluster_nodes()`
- Extracts and caches TPU, TPU QUIC, and Gossip addresses
- Auto-refreshes cache (default: 5 minutes)
- Thread-safe with `Arc<RwLock<>>`

#### Key Types

```rust
pub struct TpuContactInfo {
    pub pubkey: Pubkey,
    pub tpu: SocketAddr,        // UDP TPU address
    pub tpu_quic: SocketAddr,   // QUIC TPU address
    pub gossip: Option<SocketAddr>,
}

pub struct LeaderResolver {
    // Internal fields...
}
```

#### API

```rust
// Create resolver
let rpc_client = Arc::new(RpcClient::new(rpc_url));
let resolver = LeaderResolver::new(rpc_client);

// Get contact info for a validator
let contact_info = resolver.get_contact_info(&validator_pubkey).await?;
println!("TPU QUIC: {}", contact_info.tpu_quic);

// Get QUIC address directly
let quic_addr = resolver.get_tpu_quic_address(&validator_pubkey).await?;

// List all validators
let validators = resolver.get_all_validators().await;

// Cache statistics
let size = resolver.cache_size().await;
```

### 2. TpuConnectionManager (`src/tpu_connection_manager.rs`)

Manages a pool of QUIC connections to TPU leaders with pre-warming support.

#### Features
- QUIC connection pool using `quinn` library
- Pre-warming: establishes connections 2-4 slots ahead
- Automatic connection reuse and cleanup
- Configurable pool size
- Thread-safe connection management

#### Key Types

```rust
pub struct PrewarmConfig {
    pub slots_ahead_min: u64,  // Start warming N slots ahead
    pub slots_ahead_max: u64,  // Warm up to M slots ahead
    pub max_connections: usize, // Maximum pool size
}

pub struct TpuConnectionManager {
    // Internal fields...
}
```

#### API

```rust
// Create manager
let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));
let manager = TpuConnectionManager::new(leader_resolver).await?;

// Or with custom configuration
let config = PrewarmConfig {
    slots_ahead_min: 2,
    slots_ahead_max: 4,
    max_connections: 20,
};
let manager = TpuConnectionManager::with_config(leader_resolver, config).await?;

// Get connection (creates if needed)
let connection = manager.get_connection(&validator_pubkey).await?;

// Pre-warm connections for upcoming leaders
let upcoming = vec![
    (leader_pubkey_1, slot_1000),
    (leader_pubkey_2, slot_1001),
    (leader_pubkey_3, slot_1002),
];
manager.prewarm_connections(&upcoming).await;

// Connection management
let count = manager.connection_count().await;
manager.close_connection(&validator_pubkey).await;
manager.close_all_connections().await;
```

## Usage Example

```rust
use trigger::{LeaderResolver, TpuConnectionManager, PrewarmConfig};
use solana_client::rpc_client::RpcClient;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Create RPC client and resolver
    let rpc_url = "https://api.mainnet-beta.solana.com".to_string();
    let rpc_client = Arc::new(RpcClient::new(rpc_url));
    let leader_resolver = Arc::new(LeaderResolver::new(rpc_client.clone()));
    
    // Step 2: Create connection manager
    let prewarm_config = PrewarmConfig {
        slots_ahead_min: 2,
        slots_ahead_max: 4,
        max_connections: 20,
    };
    let manager = TpuConnectionManager::with_config(
        leader_resolver.clone(),
        prewarm_config
    ).await?;
    
    // Step 3: Get leader schedule and pre-warm
    let current_slot = rpc_client.get_slot()?;
    let leader_schedule = rpc_client.get_leader_schedule(None)?;
    
    // Find upcoming leaders
    let mut upcoming_leaders = Vec::new();
    for slot_offset in 2..=4 {
        let target_slot = current_slot + slot_offset;
        // Find leader for target_slot from schedule
        // ... (schedule parsing logic)
        // upcoming_leaders.push((leader_pubkey, target_slot));
    }
    
    // Pre-warm connections
    manager.prewarm_connections(&upcoming_leaders).await;
    
    // Step 4: Send transaction using pre-warmed connection
    let leader_pubkey = upcoming_leaders[0].0;
    let connection = manager.get_connection(&leader_pubkey).await?;
    
    // Use connection to send transaction via QUIC
    // ... (transaction sending logic)
    
    Ok(())
}
```

## Integration with Existing Code

The new components integrate seamlessly with the existing TPU client:

```rust
use trigger::{TpuClient, LeaderResolver, TpuConnectionManager};

// Existing UDP-based TPU client (for comparison)
let udp_tpu_client = TpuClient::new(rpc_url, Some(3))?;
let signature = udp_tpu_client.send_transaction_with_redundancy(&tx).await?;

// New QUIC-based approach
let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));
let quic_manager = TpuConnectionManager::new(leader_resolver).await?;

// Get connection for specific leader
let leader_pubkey = /* get from schedule */;
let connection = quic_manager.get_connection(&leader_pubkey).await?;

// Send transaction via QUIC
// ... (send transaction using quinn Connection)
```

## Testing

### Unit Tests

Run unit tests for individual components:

```bash
cd off-chain/components/trigger
cargo test --lib
```

### Integration Tests

Run integration tests that verify real RPC interaction:

```bash
cargo test --test tpu_connection_integration
```

Key integration tests:
- `test_leader_resolver_fetches_cluster_nodes` - Verifies cluster node fetching
- `test_connection_manager_with_real_validators` - Tests connection establishment
- `test_quic_handshake_simulation` - Complete workflow test
- `test_prewarm_connections_workflow` - Pre-warming logic verification

## Performance Considerations

### Cache Management
- Leader resolver cache refreshes every 5 minutes by default
- Adjust via `LeaderResolver::with_cache_duration()`
- Cache is thread-safe using `Arc<RwLock<>>`

### Connection Pool
- Default max connections: 20
- Automatic cleanup of stale connections
- Connection reuse reduces handshake overhead

### Pre-warming Strategy
- Connections established 2-4 slots ahead
- Reduces latency when slot leader transitions
- Configurable via `PrewarmConfig`

## Security Considerations

### Certificate Verification
Current implementation uses `SkipServerVerification` for development/testing. For production:

```rust
// TODO: Implement proper certificate verification
impl rustls::client::ServerCertVerifier for ProductionVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &Certificate,
        intermediates: &[Certificate],
        server_name: &rustls::ServerName,
        // ... verify certificates properly
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        // Implement proper verification
    }
}
```

### Connection Security
- QUIC provides built-in encryption (TLS 1.3)
- Mutual authentication should be considered for production
- Rate limiting on connection attempts

## Future Enhancements

1. **Leader Schedule Integration**
   - Automatic pre-warming based on leader schedule
   - Background task to maintain warm connections

2. **Metrics and Monitoring**
   - Connection pool statistics
   - Handshake latency tracking
   - Success/failure rates

3. **Advanced Connection Management**
   - Connection health checks
   - Automatic reconnection on failure
   - Load balancing across multiple connections

4. **Transaction Sending**
   - Direct transaction submission via QUIC
   - Batch submission optimization
   - Retry logic with connection fallback

## Dependencies

New dependencies added in `Cargo.toml`:

```toml
solana-connection-cache = "1.18"
quinn = "0.10"
rustls = "0.21"
```

## References

- [Solana TPU Client Documentation](https://docs.solana.com/developing/clients/jsonrpc-api#gettpu)
- [QUIC Protocol (RFC 9000)](https://www.rfc-editor.org/rfc/rfc9000.html)
- [Quinn QUIC Implementation](https://docs.rs/quinn/latest/quinn/)
- [Solana Leader Schedule](https://docs.solana.com/developing/clients/jsonrpc-api#getleaderschedule)

## Success Criteria ✓

All requirements from the issue have been met:

✅ **Leader Resolver**
- Fetches leader identity and Contact Info (IP, TPU Port, TPU QUIC Port)
- Implements cache mapping Pubkey -> SocketAddr
- Auto-refresh logic included

✅ **Connection Manager**
- Manages QUIC connection pool using quinn
- Pre-warming: connects to leaders 2-4 slots ahead
- Interface: `get_connection(leader_pubkey) -> Option<Connection>`

✅ **Test Coverage**
- Unit tests for cluster node fetching and IP resolution
- Integration test establishing QUIC handshake
- Pool management and lifecycle tests

## License

Same as parent project.
