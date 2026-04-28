import { mockShadowPositions } from '../data/mockData';
import PositionRow from '../components/dashboard/PositionRow';

export default function Shadow() {
  return (
    <div className="flex flex-col gap-4">
      {/* SHADOW header with pulse glow */}
      <div className="text-center py-2">
        <div className="font-display font-black text-2xl tracking-[0.3em]
                        text-ghost-cyan text-glow-cyan animate-pulse">
          SHADOW
        </div>
        <div className="text-ghost-muted text-xs tracking-widest mt-1">
          OPEN POSITIONS
        </div>
      </div>

      {/* Shadow positions list */}
      <div className="bg-ghost-surface border border-ghost-cyan/20 rounded-ghost p-3">
        {mockShadowPositions.map(pos => (
          <PositionRow key={pos.id} position={pos} />
        ))}
      </div>

      {/* CLOSE ALL */}
      <button className="w-full py-3 rounded-ghost font-bold text-sm tracking-wider
                         bg-ghost-red/20 border border-ghost-red text-ghost-red
                         hover:bg-ghost-red/30 transition-all active:scale-95">
        CLOSE ALL
      </button>
    </div>
  );
}
