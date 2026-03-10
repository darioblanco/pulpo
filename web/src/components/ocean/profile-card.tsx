import type { OctopusEntity } from './engine/world';

interface ProfileCardProps {
  octopus: OctopusEntity;
  screenX: number;
  screenY: number;
  onClose: () => void;
}

const STATUS_COLORS: Record<string, string> = {
  running: '#a78bfa',
  creating: '#60a5fa',
  stale: '#fbbf24',
  completed: '#34d399',
  dead: '#f87171',
};

export function ProfileCard({ octopus, screenX, screenY, onClose }: ProfileCardProps) {
  const color = STATUS_COLORS[octopus.status] ?? '#94a3b8';

  // Position card near click, clamped to viewport
  const cardW = 240;
  const cardH = 220;
  const left = Math.max(10, Math.min(screenX + 12, window.innerWidth - cardW - 10));
  const top = Math.max(10, Math.min(screenY - cardH / 2, window.innerHeight - cardH - 10));

  return (
    <div className="fixed inset-0 z-50" onClick={onClose} data-testid="profile-card-backdrop">
      <div
        className="absolute"
        style={{ left, top, width: cardW }}
        onClick={(e) => e.stopPropagation()}
        data-testid="profile-card"
      >
        <div
          className="rounded-sm p-4 font-mono text-sm shadow-xl"
          style={{
            backgroundColor: '#0c1929',
            border: '2px solid #2a5a80',
            boxShadow: 'inset 0 0 0 1px #1a3a55, 0 8px 32px rgba(0,0,0,0.5)',
            imageRendering: 'pixelated',
          }}
        >
          {/* Header */}
          <div className="mb-3 flex items-center gap-2">
            <img
              src={`/sprites/ui/icon-${octopus.provider}.png`}
              alt={octopus.provider}
              className="h-5 w-5"
              style={{ imageRendering: 'pixelated' }}
            />
            <span className="truncate font-bold text-white">{octopus.name}</span>
          </div>

          {/* Status */}
          <div className="mb-3 flex items-center gap-2">
            <img
              src={`/sprites/status/${octopus.status}.png`}
              alt={octopus.status}
              className="h-3 w-3"
              style={{ imageRendering: 'pixelated' }}
            />
            <span style={{ color }}>{octopus.status}</span>
            {octopus.waitingForInput && (
              <span className="text-xs text-amber-400">awaiting input</span>
            )}
          </div>

          {/* Details */}
          <div className="space-y-1.5 text-xs text-slate-400">
            <div>
              Provider: <span className="text-slate-300">{octopus.provider}</span>
            </div>
            {octopus.ink && (
              <div>
                Ink: <span style={{ color }}>{octopus.ink}</span>
              </div>
            )}
            <div>
              Node: <span className="text-slate-300">{octopus.nodeName}</span>
            </div>
          </div>

          {/* Actions */}
          <div className="mt-4 flex gap-2">
            <a
              href={`/session/${octopus.name}`}
              className="rounded bg-slate-700 px-2.5 py-1 text-xs text-slate-200 hover:bg-slate-600"
            >
              View Logs
            </a>
          </div>
        </div>
      </div>
    </div>
  );
}
