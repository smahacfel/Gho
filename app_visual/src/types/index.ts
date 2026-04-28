export interface Position {
  id: string;
  shortAddress: string;
  pnlPercent: number;
  isShadow?: boolean;
}

export interface BotStats {
  winRatio: number;       // 0-100
  totalPnlSol: number;    // can be negative
  walletAddress: string;
  walletBalance: number;  // in SOL
  isRunning: boolean;
}

export interface GatekeeperConfig {
  min_tx_count: number;
  min_unique_signers: number;
  min_buy_count: number;
  min_interval_cv: number;
  max_burst_ratio: number;
  max_avg_interval_ms: number;
  max_avg_interval_ms_2: number;
  max_dev_tx_ratio: number;
  max_dev_volume_ratio: number;
  min_dev_volume_ratio: number;
  reject_on_dev_sell: boolean;
  max_single_tx_price_impact: number;
  max_single_sell_impact_pct: number;
  min_bonding_progress_pct: number;
  min_market_cap_sol: number;
}
