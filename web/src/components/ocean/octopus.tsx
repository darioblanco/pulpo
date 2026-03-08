interface OctopusProps {
  x: number;
  y: number;
  status: string;
  name: string;
  provider?: string;
  ink?: string;
  waitingForInput?: boolean;
}

const statusColors: Record<string, string> = {
  running: '#a78bfa', // violet
  creating: '#60a5fa', // blue
  stale: '#fbbf24', // amber
  completed: '#34d399', // emerald
  dead: '#f87171', // red
};

export function Octopus({ x, y, status, name, provider, ink, waitingForInput }: OctopusProps) {
  const color = statusColors[status] ?? '#94a3b8';

  return (
    <g
      data-testid={`octopus-${name}`}
      className={`octopus-${status}`}
      transform={`translate(${x}, ${y})`}
    >
      <title>{name}</title>

      {/* Head */}
      <ellipse cx={0} cy={-8} rx={14} ry={12} fill={color} opacity={0.9} />

      {/* Eyes */}
      <circle cx={-5} cy={-10} r={3} fill="#0f172a" />
      <circle cx={5} cy={-10} r={3} fill="#0f172a" />
      <circle cx={-4} cy={-11} r={1.2} fill="white" />
      <circle cx={6} cy={-11} r={1.2} fill="white" />

      {/* Tentacles */}
      <path
        d={`M-12,2 Q-16,18 -10,22`}
        stroke={color}
        strokeWidth={2.5}
        fill="none"
        opacity={0.7}
        className="tentacle t1"
      />
      <path
        d={`M-7,4 Q-10,20 -4,24`}
        stroke={color}
        strokeWidth={2.5}
        fill="none"
        opacity={0.7}
        className="tentacle t2"
      />
      <path
        d={`M0,5 Q0,22 0,26`}
        stroke={color}
        strokeWidth={2.5}
        fill="none"
        opacity={0.7}
        className="tentacle t3"
      />
      <path
        d={`M7,4 Q10,20 4,24`}
        stroke={color}
        strokeWidth={2.5}
        fill="none"
        opacity={0.7}
        className="tentacle t4"
      />
      <path
        d={`M12,2 Q16,18 10,22`}
        stroke={color}
        strokeWidth={2.5}
        fill="none"
        opacity={0.7}
        className="tentacle t5"
      />

      {/* Waiting for input indicator — speech bubble */}
      {waitingForInput && (
        <g data-testid="waiting-indicator">
          <ellipse cx={18} cy={-24} rx={6} ry={4} fill="white" opacity={0.9} />
          <circle cx={14} cy={-18} r={1.5} fill="white" opacity={0.7} />
          <circle cx={12} cy={-14} r={1} fill="white" opacity={0.5} />
          <text x={18} y={-22} textAnchor="middle" fontSize={6} fill="#0f172a">
            ?
          </text>
        </g>
      )}

      {/* Name label */}
      <text y={32} textAnchor="middle" fontSize={9} fontFamily="monospace" fill="#94a3b8">
        {name.length > 12 ? `${name.slice(0, 11)}…` : name}
      </text>

      {/* Provider badge */}
      {provider && (
        <text y={42} textAnchor="middle" fontSize={7} fontFamily="monospace" fill="#64748b">
          {provider}
        </text>
      )}

      {/* Ink badge */}
      {ink && (
        <text
          y={provider ? 50 : 42}
          textAnchor="middle"
          fontSize={7}
          fontFamily="monospace"
          fill={color}
          opacity={0.7}
        >
          {ink}
        </text>
      )}
    </g>
  );
}
