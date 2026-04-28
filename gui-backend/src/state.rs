//! Application state management with atomic runtime control

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;

/// System operating mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SystemMode {
    /// System is running normally
    Running = 0,
    /// System is paused (not processing new candidates)
    Paused = 1,
    /// System is stopped (shutdown requested)
    Stopped = 2,
}

impl From<u8> for SystemMode {
    fn from(val: u8) -> Self {
        match val {
            0 => SystemMode::Running,
            1 => SystemMode::Paused,
            2 => SystemMode::Stopped,
            _ => SystemMode::Stopped,
        }
    }
}

/// System status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    /// Current system mode
    pub mode: SystemMode,

    /// System uptime in seconds
    pub uptime_secs: u64,

    /// Number of transactions sent
    pub transactions_sent: u64,

    /// Number of transactions confirmed
    pub transactions_confirmed: u64,

    /// Number of active positions
    pub active_positions: u32,

    /// Last update timestamp (Unix timestamp)
    pub last_update: u64,
}

/// Portfolio position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// Token mint address
    pub mint: String,

    /// Amount of tokens held
    pub amount: u64,

    /// Entry price in lamports
    pub entry_price: u64,

    /// Current price in lamports (if available)
    pub current_price: Option<u64>,

    /// Profit/loss in lamports
    pub pnl: i64,

    /// Timestamp when position was opened
    pub opened_at: u64,
}

/// Portfolio state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    /// SOL balance in lamports
    pub sol_balance: u64,

    /// List of open positions
    pub positions: Vec<Position>,

    /// Total portfolio value in lamports
    pub total_value: u64,

    /// Total profit/loss in lamports
    pub total_pnl: i64,
}

impl Default for Portfolio {
    fn default() -> Self {
        Self {
            sol_balance: 0,
            positions: Vec::new(),
            total_value: 0,
            total_pnl: 0,
        }
    }
}

/// User-configurable settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Position size in lamports for new trades
    pub position_size_lamports: u64,

    /// Jito tip amount in lamports
    pub jito_tip_lamports: u64,

    /// Maximum slippage tolerance (0.0 - 1.0)
    pub max_slippage: f64,

    /// Enable Jito bundles
    pub enable_jito: bool,

    /// Auto-calculate Jito tips based on transaction value
    /// If true, jito_tip_lamports is used as minimum tip
    /// If false, jito_tip_lamports is used as fixed tip
    pub auto_jito_tip: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            position_size_lamports: 100_000_000, // 0.1 SOL
            jito_tip_lamports: 10_000,           // 0.00001 SOL
            max_slippage: 0.01,                  // 1%
            enable_jito: false,
            auto_jito_tip: true,
        }
    }
}

impl Settings {
    /// Convert Settings to RuntimeConfig
    pub fn to_runtime_config(&self) -> crate::runtime_config::RuntimeConfig {
        crate::runtime_config::RuntimeConfig {
            position_size_lamports: self.position_size_lamports,
            jito_tip_lamports: self.jito_tip_lamports,
            max_slippage: self.max_slippage,
            enable_jito: self.enable_jito,
            auto_jito_tip: self.auto_jito_tip,
        }
    }
}

/// Application state shared across handlers
pub struct AppState {
    /// Atomic system mode for thread-safe runtime control
    mode: Arc<AtomicU8>,

    /// System status (protected by RwLock)
    status: Arc<RwLock<SystemStatus>>,

    /// Portfolio state (protected by RwLock)
    portfolio: Arc<RwLock<Portfolio>>,

    /// User settings (protected by RwLock)
    settings: Arc<RwLock<Settings>>,

    /// Broadcast channel for live updates
    update_tx: broadcast::Sender<StateUpdate>,

    /// System start time
    start_time: std::time::Instant,
}

/// State update event for WebSocket broadcasts
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StateUpdate {
    /// System status changed
    StatusUpdate { status: SystemStatus },

    /// Portfolio changed
    PortfolioUpdate { portfolio: Portfolio },

    /// Settings changed
    SettingsUpdate { settings: Settings },

    /// System mode changed
    ModeUpdate { mode: SystemMode },
}

impl AppState {
    /// Create new application state
    pub fn new() -> Self {
        let (update_tx, _) = broadcast::channel(100);

        let initial_status = SystemStatus {
            mode: SystemMode::Running,
            uptime_secs: 0,
            transactions_sent: 0,
            transactions_confirmed: 0,
            active_positions: 0,
            last_update: Self::current_timestamp(),
        };

        Self {
            mode: Arc::new(AtomicU8::new(SystemMode::Running as u8)),
            status: Arc::new(RwLock::new(initial_status)),
            portfolio: Arc::new(RwLock::new(Portfolio::default())),
            settings: Arc::new(RwLock::new(Settings::default())),
            update_tx,
            start_time: std::time::Instant::now(),
        }
    }

    /// Get current system mode
    pub fn get_mode(&self) -> SystemMode {
        self.mode.load(Ordering::SeqCst).into()
    }

    /// Set system mode atomically
    pub fn set_mode(&self, mode: SystemMode) {
        self.mode.store(mode as u8, Ordering::SeqCst);

        // Broadcast mode change
        let _ = self.update_tx.send(StateUpdate::ModeUpdate { mode });

        // Update status
        if let Ok(mut status) = self.status.write() {
            status.mode = mode;
            status.last_update = Self::current_timestamp();
        }
    }

    /// Check if system is running
    pub fn is_running(&self) -> bool {
        matches!(self.get_mode(), SystemMode::Running)
    }

    /// Check if system is paused
    pub fn is_paused(&self) -> bool {
        matches!(self.get_mode(), SystemMode::Paused)
    }

    /// Check if system is stopped
    pub fn is_stopped(&self) -> bool {
        matches!(self.get_mode(), SystemMode::Stopped)
    }

    /// Get system status
    pub fn get_status(&self) -> SystemStatus {
        let mut status = self.status.read().unwrap().clone();
        status.uptime_secs = self.start_time.elapsed().as_secs();
        status.mode = self.get_mode();
        status.last_update = Self::current_timestamp();
        status
    }

    /// Update transaction counters
    pub fn update_transaction_stats(&self, sent: u64, confirmed: u64) {
        if let Ok(mut status) = self.status.write() {
            status.transactions_sent = sent;
            status.transactions_confirmed = confirmed;
            status.last_update = Self::current_timestamp();
        }

        // Broadcast status update
        let _ = self.update_tx.send(StateUpdate::StatusUpdate {
            status: self.get_status(),
        });
    }

    /// Get portfolio
    pub fn get_portfolio(&self) -> Portfolio {
        self.portfolio.read().unwrap().clone()
    }

    /// Update portfolio
    pub fn update_portfolio(&self, portfolio: Portfolio) {
        if let Ok(mut p) = self.portfolio.write() {
            *p = portfolio.clone();
        }

        // Update active positions in status
        if let Ok(mut status) = self.status.write() {
            status.active_positions = portfolio.positions.len() as u32;
        }

        // Broadcast portfolio update
        let _ = self
            .update_tx
            .send(StateUpdate::PortfolioUpdate { portfolio });
    }

    /// Get settings
    pub fn get_settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// Update settings
    pub fn update_settings(&self, settings: Settings) {
        if let Ok(mut s) = self.settings.write() {
            *s = settings.clone();
        }

        // Broadcast settings update
        let _ = self
            .update_tx
            .send(StateUpdate::SettingsUpdate { settings });
    }

    /// Get settings as runtime config
    pub fn get_runtime_config(&self) -> crate::runtime_config::RuntimeConfig {
        self.get_settings().to_runtime_config()
    }

    /// Subscribe to state updates
    pub fn subscribe(&self) -> broadcast::Receiver<StateUpdate> {
        self.update_tx.subscribe()
    }

    /// Get current Unix timestamp
    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
