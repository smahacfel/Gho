import { Position, BotStats, GatekeeperConfig } from '../types';

export type { Position, BotStats, GatekeeperConfig };

export const mockStats: BotStats = {
  winRatio: 66.36,
  totalPnlSol: 3.5,
  walletAddress: "yc5545dkw3l4d....",
  walletBalance: 35,
  isRunning: true,
};

export const mockPositions: Position[] = [
  { id: "1", shortAddress: "23lsd|kfacekf3nd23lsd", pnlPercent: 8.4 },
  { id: "2", shortAddress: "3ks56macekf3nd23lsd",   pnlPercent: 8.0 },
  { id: "3", shortAddress: "56s56macekfmace3lsd",   pnlPercent: -4.0 },
];

export const mockShadowPositions: Position[] = [
  { id: "s1", shortAddress: "f93djosd93384huj8dhhs",    pnlPercent: 23.0,  isShadow: true },
  { id: "s2", shortAddress: "cweletsd93384huj8dhhs",    pnlPercent: 79.6,  isShadow: true },
  { id: "s3", shortAddress: "7788djosdhu j4hu8dhhs",    pnlPercent: -23.9, isShadow: true },
  { id: "s4", shortAddress: "9998djosdhu j4hu5532hhs",  pnlPercent: -9.9,  isShadow: true },
  { id: "s5", shortAddress: "3678djosdhu hector8dhhs",  pnlPercent: 377.0, isShadow: true },
];

export const mockConfig: GatekeeperConfig = {
  min_tx_count: 1000,
  min_unique_signers: 10,
  min_buy_count: 500,
  min_interval_cv: 0.1,
  max_burst_ratio: 5.0,
  max_avg_interval_ms: 100,
  max_avg_interval_ms_2: 500,
  max_dev_tx_ratio: 0.2,
  max_dev_volume_ratio: 0.3,
  min_dev_volume_ratio: 0.05,
  reject_on_dev_sell: false,
  max_single_tx_price_impact: 2.5,
  max_single_sell_impact_pct: 1.8,
  min_bonding_progress_pct: 5.0,
  min_market_cap_sol: 50.0,
};
