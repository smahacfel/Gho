# Ghost Launcher - Integrated Standalone Application

Ghost Launcher is an integrated standalone application that launches all Ghost system components in a single process. It provides a convenient way to run the entire Ghost trading system with a single executable and unified configuration.

## Features

- **Single Executable**: All components (Seer, Trigger, GUI Backend, DirectBuyBuilder) run in one process
- **Unified Configuration**: Configure all components via a single `config.toml` file
- **Centralized Logging**: All component logs are aggregated to a single location (file and/or console)
- **Test/Production Modes**: Easy switching between test (devnet) and production (mainnet) environments
- **Graceful Shutdown**: Ctrl+C cleanly shuts down all components
- **Component Control**: Enable/disable individual components via configuration
- **Cross-Platform**: Runs on Windows, Linux, and macOS

## Components

### 1. Seer (Pool Detection)
- Detects new pool initializations on Pump.fun and Bonk.fun
- Supports both WebSocket and gRPC connections
- Configurable filters and IPC buffering

### 2. Trigger (Transaction Sender)
- Builds and sends Ghost transactions
- N+3 redundancy for reliable transaction delivery
- Optional Jito bundle support for faster inclusion

### 3. GUI Backend (Monitoring & Control)
- REST API and WebSocket server
- Portfolio tracking and statistics
- Runtime control (pause/resume/stop)
- Settings management

### 4. DirectBuyBuilder (On-Chain Monitoring)
- Monitors DirectBuyBuilder state
- Configurable polling interval
- Optional component (disabled by default)

## Installation

### Prerequisites

- Rust 1.72 or newer
- Cargo (comes with Rust)
- (Optional) Cross-compilation tools for Windows builds

### Building from Source

1. Clone the repository:
```bash
git clone https://github.com/Mezoscope/ProjectSolanaGhost.git
cd ProjectSolanaGhost
```

2. Build the launcher:
```bash
cargo build --release --bin ghost-launcher
```

3. The executable will be at:
   - Linux/macOS: `target/release/ghost-launcher`
   - Windows: `target\release\ghost-launcher.exe`

### Building for Windows (Cross-Compilation from Linux)

Install the Windows target:
```bash
rustup target add x86_64-pc-windows-gnu
```

Build for Windows:
```bash
cargo build --release --bin ghost-launcher --target x86_64-pc-windows-gnu
```

The Windows executable will be at: `target/x86_64-pc-windows-gnu/release/ghost-launcher.exe`

## Configuration

### Secret hygiene

- Trackowany `config.toml` oraz profile w `configs/rollout/` zawierają wyłącznie bezpieczne placeholdery.
- Sekrety runtime są ładowane z procesowych env albo z lokalnego `.env` w katalogu repo.
- Nie commituj `.env`, walletów rolloutowych ani żadnych `solana/*.json` z realnymi kluczami.
- Szczegóły operacyjne są opisane w `docs/SECRET_HYGIENE_AND_ROLLOUT_PROFILES.md`.

Obsługiwane zmienne środowiskowe:

- `GHOST_SEER_GRPC_ENDPOINT`
- `GHOST_SEER_GRPC_X_TOKEN`
- `GHOST_SEER_RPC_ENDPOINT`
- `GHOST_TRIGGER_RPC_URL`
- `GHOST_TRIGGER_KEYPAIR_PATH`
- `GHOST_TRIGGER_SHADOW_RPC_URL`
- `GHOST_TRIGGER_JITO_ENDPOINT`
- `GHOST_ENV_FILE` (opcjonalna ścieżka do alternatywnego `.env`)

### Creating Configuration File

1. Copy the example configuration:
```bash
cp config.toml.example config.toml
```

2. Or generate a default configuration:
```bash
./ghost-launcher --generate-config
```

3. Edit `config.toml` to match your environment:

```toml
# Application mode: "test" or "production"
mode = "test"

[seer]
enabled = true
connection_mode = "grpc"
grpc_endpoint = "http://localhost:10000"
rpc_endpoint = "https://api.devnet.solana.com"
# ... more settings ...

[trigger]
enabled = true
rpc_url = "https://api.devnet.solana.com"
live_preflight_max_state_age_slots = 10

[trigger.tip_guard]
max_tip_absolute_sol = 0.0007
fallback_tip_sol = 0.0007
# ... more settings ...

[gui_backend]
enabled = true
bind_address = "127.0.0.1"
port = 8800

[direct_buy_monitor]
enabled = false

[logging]
level = "info"
file_enabled = true
file_path = "logs/ghost.log"
console_enabled = true
```

### Configuration Options

#### Application Mode
- `mode = "test"`: Uses devnet/testnet endpoints (safe for testing)
- `mode = "production"`: Uses mainnet endpoints (real trading with real funds)

#### Seer Component
- `enabled`: Enable/disable Seer component
- `connection_mode`: `"grpc"` or `"websocket"`
- `grpc_endpoint`: Yellowstone gRPC endpoint
- `rpc_endpoint`: Solana RPC endpoint
- `enable_pumpfun`/`enable_bonkfun`: Platform filters
- `metrics_port`: Prometheus metrics port

#### Trigger Component
- `enabled`: Enable/disable Trigger component
- `rpc_url`: Solana RPC endpoint for sending transactions
- `live_preflight_max_state_age_slots`: Max acceptable AccountStateCore staleness for live BUY preflight
- `live_exit_take_profit_pct` / `live_exit_stop_loss_pct`: Configurable live SELL thresholds
- `[trigger.tip_guard]`: Local tip clamp used only when the sender path needs a safe fallback
- `metrics_port`: Prometheus metrics port

#### GUI Backend Component
- `enabled`: Enable/disable GUI Backend
- `bind_address`: Network interface to bind to
- `port`: HTTP server port

#### DirectBuyBuilder Component
- `enabled`: Enable/disable monitoring
- `rpc_endpoint`: RPC endpoint for queries
- `program_id`: DirectBuyBuilder ID to monitor
- `poll_interval_secs`: Polling frequency

#### Logging Configuration
- `level`: Log level (`"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`)
- `file_enabled`: Enable file logging
- `file_path`: Path to log file
- `console_enabled`: Enable console logging
- `json_format`: Use JSON format for structured logs

## Usage

### Running the Launcher

With default configuration file (`config.toml` in current directory):
```bash
./ghost-launcher
```

With custom configuration file:
```bash
./ghost-launcher /path/to/config.toml
```

### Rollout profiles

Repo zawiera gotowe profile rolloutowe:

- `configs/rollout/paper-burnin.toml`
- `configs/rollout/dual-micro-live.toml`
- `configs/rollout/future-live.toml`

Po PR-5 do użycia operacyjnego dopuszczony jest wyłącznie `paper-burnin`. Pozostałe dwa profile są przygotowane konfiguracyjnie, ale mają twardą blokadę proceduralną do kolejnych PR-ów. Mały config, duża odpowiedzialność — klasyka gatunku.

### Windows Usage

1. Download or build `ghost-launcher.exe`
2. Place `config.toml` in the same directory as the executable
3. Double-click `ghost-launcher.exe` or run from command prompt:
```cmd
ghost-launcher.exe
```

### Stopping the Launcher

Press `Ctrl+C` to gracefully shutdown all components.

## Accessing the GUI Backend

If GUI Backend is enabled, you can access it at:
- REST API: `http://127.0.0.1:8800` (or your configured address/port)
- WebSocket: `ws://127.0.0.1:8800/ws`

### Available Endpoints

- `GET /health` - Health check
- `GET /status` - System status
- `GET /portfolio` - Portfolio information
- `GET /settings` - Current settings
- `POST /settings` - Update settings
- `POST /control/pause` - Pause trading
- `POST /control/resume` - Resume trading
- `POST /control/stop` - Stop system

## Logs

Logs are written to both console and file (if enabled in configuration).

Default log location: `logs/ghost.log`

The log file rotates daily automatically.

### Example Log Output

```
2025-11-18T23:00:00Z INFO ghost_launcher: === Ghost Launcher v0.1.0 ===
2025-11-18T23:00:00Z INFO ghost_launcher: Mode: Test
2025-11-18T23:00:00Z INFO ghost_launcher: Starting Seer component...
2025-11-18T23:00:01Z INFO seer: Seer: Initializing component
2025-11-18T23:00:01Z INFO seer: Seer: Configuration loaded
2025-11-18T23:00:02Z INFO ghost_launcher: Starting Trigger component...
2025-11-18T23:00:02Z INFO trigger: Trigger: Initializing component
2025-11-18T23:00:03Z INFO ghost_launcher: Starting GUI Backend component...
2025-11-18T23:00:03Z INFO gui_backend: GUI Backend: Server available at http://127.0.0.1:8800
2025-11-18T23:00:03Z INFO ghost_launcher: All components started successfully
```

## Monitoring

### Prometheus Metrics

Each component exposes Prometheus metrics on its configured port:
- Seer: Port 9090 (default)
- Trigger: Port 9091 (default)

Access metrics at: `http://localhost:PORT/metrics`

### System Status

Check system status via GUI Backend:
```bash
curl http://localhost:8800/status
```

## Troubleshooting

### Component Won't Start

1. Check the configuration file for syntax errors
2. Verify all endpoints are accessible
3. Check log files for detailed error messages
4. Ensure required ports are not in use

### Connection Errors

1. Verify RPC/gRPC endpoints are correct and accessible
2. Check firewall settings
3. For gRPC, ensure authentication tokens are valid (if required)

### High Memory/CPU Usage

1. Reduce `ipc_buffer_size` in Seer configuration
2. Adjust log level to `"warn"` or `"error"`
3. Disable unused components

### Logs Not Appearing

1. Check `logging.file_enabled` is `true`
2. Ensure log directory exists and is writable
3. Check disk space

## Production Deployment

### Security Considerations

1. **Never commit config.toml with sensitive data** (RPC keys, auth tokens) to version control
2. Use environment-specific configuration files
3. Restrict file permissions on `config.toml`: `chmod 600 config.toml`
4. Use secure RPC endpoints (HTTPS/WSS)
5. Enable firewall rules to restrict access to component ports

### Recommended Production Settings

```toml
mode = "production"

[seer]
grpc_endpoint = "https://your-secure-grpc-endpoint"
rpc_endpoint = "https://api.mainnet-beta.solana.com"
grpc_auth_token = "your-secure-token"

[trigger]
rpc_url = "https://api.mainnet-beta.solana.com"
live_preflight_max_state_age_slots = 10
live_exit_take_profit_pct = 0.30
live_exit_stop_loss_pct = 0.30

[trigger.tip_guard]
max_tip_absolute_sol = 0.0007
fallback_tip_sol = 0.0007

[logging]
level = "info"
file_enabled = true
json_format = true  # For structured logging and analysis
```

### Running as a Windows Service

You can use tools like [NSSM](https://nssm.cc/) to run Ghost Launcher as a Windows service:

```cmd
nssm install GhostLauncher "C:\path\to\ghost-launcher.exe"
nssm set GhostLauncher AppDirectory "C:\path\to\config\directory"
nssm start GhostLauncher
```

## Development

### Project Structure

```
ghost-launcher/
├── Cargo.toml              # Package configuration
├── src/
│   ├── main.rs             # Main entry point
│   ├── config.rs           # Configuration loading
│   └── components/         # Component wrappers
│       ├── mod.rs
│       ├── seer.rs
│       ├── trigger.rs
│       ├── gui_backend.rs
│       └── direct_buy_monitor.rs
```

### Adding New Components

1. Create a new module in `src/components/`
2. Implement the `run()` function with signature:
   ```rust
   pub async fn run(
       config: YourComponentConfig,
       shutdown_rx: broadcast::Receiver<()>,
   ) -> Result<()>
   ```
3. Add configuration struct in `src/config.rs`
4. Register in `src/main.rs`

## Support

For issues, questions, or contributions:
- GitHub Issues: https://github.com/Mezoscope/ProjectSolanaGhost/issues
- Documentation: See main README.md

## License

See the main project LICENSE file.
