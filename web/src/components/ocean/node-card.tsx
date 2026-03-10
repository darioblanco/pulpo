import type { NodeLandmark } from './engine/world';

interface NodeCardProps {
  node: NodeLandmark;
  screenX: number;
  screenY: number;
  onClose: () => void;
}

export function NodeCard({ node, screenX, screenY, onClose }: NodeCardProps) {
  // Position card near click, clamped to viewport
  const cardW = 220;
  const cardH = 160;
  const left = Math.max(10, Math.min(screenX + 12, window.innerWidth - cardW - 10));
  const top = Math.max(10, Math.min(screenY - cardH / 2, window.innerHeight - cardH - 10));

  const statusColor =
    node.status === 'online' ? '#34d399' : node.status === 'offline' ? '#f87171' : '#94a3b8';
  const statusLabel =
    node.status === 'online' ? 'Online' : node.status === 'offline' ? 'Offline' : 'Unknown';

  return (
    <div className="fixed inset-0 z-50" onClick={onClose} data-testid="node-card-backdrop">
      <div
        className="absolute"
        style={{ left, top, width: cardW }}
        onClick={(e) => e.stopPropagation()}
        data-testid="node-card"
      >
        <div
          className="rounded-sm p-4 font-mono text-sm shadow-xl"
          style={{
            backgroundColor: '#0c1929',
            border: `2px solid ${node.color}44`,
            boxShadow: `inset 0 0 0 1px ${node.color}22, 0 8px 32px rgba(0,0,0,0.5)`,
            imageRendering: 'pixelated',
          }}
        >
          {/* Header */}
          <div className="mb-3 flex items-center gap-2">
            <span
              className="inline-block h-3 w-3 rounded-full"
              style={{ backgroundColor: statusColor }}
              data-testid="node-status-dot"
            />
            <span className="truncate font-bold text-white">{node.name}</span>
            {node.isLocal && <span className="text-xs text-slate-500">(local)</span>}
          </div>

          {/* Status */}
          <div className="mb-3 flex items-center gap-2">
            <span style={{ color: statusColor }}>{statusLabel}</span>
          </div>

          {/* Details */}
          <div className="space-y-1.5 text-xs text-slate-400">
            <div>
              Sessions:{' '}
              <span className="text-slate-300" data-testid="node-session-count">
                {node.sessionCount}
              </span>
            </div>
            <div>
              Type:{' '}
              <span className="text-slate-300">{node.isLocal ? 'Local node' : 'Peer node'}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
