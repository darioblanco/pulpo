import type { Session, NodeInfo, PeerInfo } from '@/api/types';
import { Octopus } from './octopus';

interface OceanCanvasProps {
  localNode: NodeInfo;
  localSessions: Session[];
  peers: PeerInfo[];
  peerSessions: Record<string, Session[]>;
}

const ISLAND_WIDTH = 160;
const ISLAND_GAP = 40;
const ISLAND_Y = 60;
const OCEAN_TOP = 120;
const OCTOPUS_SPACING_X = 70;
const OCTOPUS_SPACING_Y = 80;
const OCTOPUS_PER_ROW = 3;
const MIN_HEIGHT = 400;

interface Island {
  name: string;
  status: 'online' | 'offline' | 'unknown';
  sessions: Session[];
  x: number;
}

export function OceanCanvas({ localNode, localSessions, peers, peerSessions }: OceanCanvasProps) {
  // Build island list
  const islands: Island[] = [];

  // Local node is always first
  islands.push({
    name: localNode.name,
    status: 'online',
    sessions: localSessions,
    x: 0,
  });

  // Add peer islands
  for (const peer of peers) {
    islands.push({
      name: peer.name,
      status: peer.status,
      sessions: peerSessions[peer.name] ?? [],
      x: 0,
    });
  }

  // Calculate x positions
  const totalWidth = islands.length * ISLAND_WIDTH + (islands.length - 1) * ISLAND_GAP;
  const startX = Math.max(40, 0);
  for (let i = 0; i < islands.length; i++) {
    islands[i].x = startX + i * (ISLAND_WIDTH + ISLAND_GAP) + ISLAND_WIDTH / 2;
  }

  // Calculate canvas dimensions
  const maxSessions = Math.max(1, ...islands.map((is) => is.sessions.length));
  const rows = Math.ceil(maxSessions / OCTOPUS_PER_ROW);
  const canvasHeight = Math.max(MIN_HEIGHT, OCEAN_TOP + rows * OCTOPUS_SPACING_Y + 80);
  const canvasWidth = Math.max(totalWidth + 80, 400);

  const totalSessions = islands.reduce((sum, is) => sum + is.sessions.length, 0);

  return (
    <div className="ocean-container relative w-full overflow-x-auto">
      <svg
        data-testid="ocean-canvas"
        viewBox={`0 0 ${canvasWidth} ${canvasHeight}`}
        className="w-full"
        style={{ minHeight: `${Math.min(canvasHeight, 600)}px` }}
      >
        {/* Ocean gradient background */}
        <defs>
          <linearGradient id="ocean-bg" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#0c1929" />
            <stop offset="50%" stopColor="#0a1628" />
            <stop offset="100%" stopColor="#071320" />
          </linearGradient>
          <radialGradient id="island-glow" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="#1e3a5f" stopOpacity={0.4} />
            <stop offset="100%" stopColor="#0c1929" stopOpacity={0} />
          </radialGradient>
        </defs>

        <rect width={canvasWidth} height={canvasHeight} fill="url(#ocean-bg)" />

        {/* Water surface line */}
        <path
          d={`M0,${OCEAN_TOP - 10} Q${canvasWidth / 4},${OCEAN_TOP - 20} ${canvasWidth / 2},${OCEAN_TOP - 10} T${canvasWidth},${OCEAN_TOP - 10}`}
          stroke="#1e3a5f"
          strokeWidth={1}
          fill="none"
          opacity={0.5}
        />

        {/* Islands */}
        {islands.map((island) => (
          <g key={island.name}>
            {/* Island glow */}
            <ellipse
              cx={island.x}
              cy={ISLAND_Y}
              rx={ISLAND_WIDTH / 2}
              ry={30}
              fill="url(#island-glow)"
            />

            {/* Island body */}
            <ellipse
              cx={island.x}
              cy={ISLAND_Y}
              rx={ISLAND_WIDTH / 2.5}
              ry={16}
              fill={island.status === 'online' ? '#1a3550' : '#1a1a2e'}
              stroke={island.status === 'online' ? '#2a5a80' : '#2a2a3e'}
              strokeWidth={1}
            />

            {/* Island label */}
            <text
              x={island.x}
              y={ISLAND_Y + 5}
              textAnchor="middle"
              fontSize={11}
              fontFamily="monospace"
              fontWeight="bold"
              fill={island.status === 'online' ? '#7dd3fc' : '#64748b'}
            >
              {island.name}
            </text>

            {/* Status dot */}
            <circle
              cx={island.x + ISLAND_WIDTH / 2.5 - 8}
              cy={ISLAND_Y - 8}
              r={3}
              fill={
                island.status === 'online'
                  ? '#34d399'
                  : island.status === 'offline'
                    ? '#f87171'
                    : '#94a3b8'
              }
            />

            {/* Octopuses for this island */}
            {island.sessions.map((session, idx) => {
              const col = idx % OCTOPUS_PER_ROW;
              const row = Math.floor(idx / OCTOPUS_PER_ROW);
              const ox =
                island.x +
                (col - (Math.min(island.sessions.length, OCTOPUS_PER_ROW) - 1) / 2) *
                  OCTOPUS_SPACING_X;
              const oy = OCEAN_TOP + 30 + row * OCTOPUS_SPACING_Y;

              return (
                <Octopus
                  key={session.id}
                  x={ox}
                  y={oy}
                  status={session.status}
                  name={session.name}
                  provider={session.provider}
                  ink={session.ink ?? undefined}
                  waitingForInput={session.waiting_for_input}
                />
              );
            })}
          </g>
        ))}

        {/* Empty state */}
        {totalSessions === 0 && (
          <text
            x={canvasWidth / 2}
            y={canvasHeight / 2}
            textAnchor="middle"
            fontSize={14}
            fill="#64748b"
            fontFamily="monospace"
          >
            No active sessions — the ocean is calm
          </text>
        )}
      </svg>
    </div>
  );
}
