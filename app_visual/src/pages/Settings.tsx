import { useState } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import NumericField from '../components/settings/NumericField';
import ToggleField  from '../components/settings/ToggleField';
import { mockConfig } from '../data/mockData';
import { GatekeeperConfig } from '../types';

export default function Settings() {
  const [config, setConfig] = useState<GatekeeperConfig>(mockConfig);
  const [page, setPage] = useState(0); // 0 = card 1, 1 = card 2

  const update = (key: keyof GatekeeperConfig, value: number | boolean) =>
    setConfig(prev => ({ ...prev, [key]: value }));

  const CARD_1: [string, string][] = [
    ['min_tx_count',          'min_unique_signers'],
    ['min_buy_count',         'min_interval_cv'],
    ['max_burst_ratio',       'max_avg_interval_ms'],
    ['max_avg_interval_ms_2', 'max_dev_tx_ratio'],
  ];
  const CARD_2: [string, string][] = [
    ['max_dev_volume_ratio',       'min_dev_volume_ratio'],
    ['max_single_tx_price_impact', 'max_single_sell_impact_pct'],
    ['min_bonding_progress_pct',   'min_market_cap_sol'],
  ];

  const rows = page === 0 ? CARD_1 : CARD_2;

  return (
    <div className="flex flex-col gap-4">
      {/* Parameters card */}
      <div className="bg-ghost-surface border border-ghost-border rounded-ghost p-4">
        <div className="grid gap-3">
          {rows.map(([left, right]) => (
            <div key={left} className="grid grid-cols-2 gap-3">
              <NumericField
                label={left}
                value={config[left as keyof GatekeeperConfig] as number}
                onChange={v => update(left as keyof GatekeeperConfig, v)}
              />
              {right && (
                <NumericField
                  label={right}
                  value={config[right as keyof GatekeeperConfig] as number}
                  onChange={v => update(right as keyof GatekeeperConfig, v)}
                />
              )}
            </div>
          ))}

          {/* Toggle — only on card 1 */}
          {page === 0 && (
            <ToggleField
              label="reject_on_dev_sell"
              value={config.reject_on_dev_sell}
              onChange={v => update('reject_on_dev_sell', v)}
            />
          )}
        </div>
      </div>

      {/* Pagination + SAVE */}
      <div className="flex items-center justify-between">
        <button
          onClick={() => setPage(p => Math.max(0, p - 1))}
          disabled={page === 0}
          className="text-ghost-cyan disabled:opacity-20 p-2"
        >
          <ChevronLeft size={20} />
        </button>

        <button className="flex-1 mx-3 py-3 rounded-ghost font-bold text-sm
                           bg-ghost-cyan/20 border border-ghost-cyan text-ghost-cyan
                           hover:bg-ghost-cyan/30 transition-all active:scale-95">
          ZAPISZ
        </button>

        <button
          onClick={() => setPage(p => Math.min(1, p + 1))}
          disabled={page === 1}
          className="text-ghost-cyan disabled:opacity-20 p-2"
        >
          <ChevronRight size={20} />
        </button>
      </div>

      {/* Page indicator dots */}
      <div className="flex justify-center gap-2">
        {[0, 1].map(i => (
          <div
            key={i}
            className={`w-2 h-2 rounded-full transition-colors ${
              i === page ? 'bg-ghost-cyan' : 'bg-ghost-border'
            }`}
          />
        ))}
      </div>
    </div>
  );
}
