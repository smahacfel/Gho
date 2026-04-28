import WinRatioGauge from '../components/dashboard/WinRatioGauge';
import PnLDisplay    from '../components/dashboard/PnLDisplay';
import WalletBadge  from '../components/dashboard/WalletBadge';
import PositionsList from '../components/dashboard/PositionsList';
import ActionButtons from '../components/dashboard/ActionButtons';
import { mockStats, mockPositions } from '../data/mockData';

export default function Dashboard() {
  return (
    <div className="flex flex-col gap-4">

      {/* Main card: Gauge + P&L */}
      <div className="bg-ghost-surface border border-ghost-cyan/30 rounded-ghost p-4
                      glow-cyan flex flex-col items-center gap-2">
        <WinRatioGauge value={mockStats.winRatio} />
        <PnLDisplay value={mockStats.totalPnlSol} />
      </div>

      {/* Wallet */}
      <WalletBadge
        address={mockStats.walletAddress}
        balance={mockStats.walletBalance}
      />

      {/* Open Positions */}
      <div className="bg-ghost-surface border border-ghost-border rounded-ghost p-3">
        <div className="text-ghost-muted text-xs uppercase tracking-widest mb-2">
          Open Positions
        </div>
        <PositionsList positions={mockPositions} />
        <button className="w-full mt-3 py-2 rounded-md bg-transparent
                           border border-ghost-red text-ghost-red text-sm font-bold
                           hover:bg-ghost-red/10 transition-colors">
          CLOSE ALL
        </button>
      </div>

      {/* START / SHUT DOWN */}
      <ActionButtons isRunning={mockStats.isRunning} />

    </div>
  );
}
