import { useMemo } from 'react';
import type { OctopusEntity } from './engine/world';
import { formatDuration } from '@/lib/utils';

interface ProfileCardProps {
  octopus: OctopusEntity;
  screenX: number;
  screenY: number;
  onClose: () => void;
  onAttach?: (sessionName: string) => void;
  onKill?: (sessionName: string) => void;
  onDelete?: (sessionName: string) => void;
}

const STATUS_COLORS: Record<string, string> = {
  active: '#a78bfa',
  creating: '#60a5fa',
  idle: '#fbbf24',
  lost: '#fbbf24',
  finished: '#34d399',
  killed: '#f87171',
};

const ENDED_STATUSES = ['finished', 'killed'];
const KILLABLE_STATUSES = ['active', 'creating'];
const DELETABLE_STATUSES = ['finished', 'killed', 'lost', 'idle'];

function shortenModel(model: string): string {
  // e.g. "claude-sonnet-4-20250514" -> "sonnet-4"
  // e.g. "gpt-4o-mini" -> "gpt-4o-mini"
  const parts = model.split('-');
  // Drop "claude-" prefix and date suffix (8+ digits)
  const filtered = parts.filter((p) => p !== 'claude' && !/^\d{8,}$/.test(p));
  return filtered.join('-') || model;
}

function truncateWorkdir(workdir: string): string {
  const segments = workdir.split('/').filter(Boolean);
  if (segments.length <= 2) return workdir;
  return `…/${segments.slice(-2).join('/')}`;
}

function relativeTime(iso: string): string {
  const diff = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (diff < 10) return 'just now';
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  const hours = Math.floor(diff / 3600);
  return `${hours}h ago`;
}

export function ProfileCard({
  octopus,
  screenX,
  screenY,
  onClose,
  onAttach,
  onKill,
  onDelete,
}: ProfileCardProps) {
  const color = STATUS_COLORS[octopus.status] ?? '#94a3b8';
  const isEnded = ENDED_STATUSES.includes(octopus.status);

  const duration = useMemo(() => {
    const dur = formatDuration(octopus.createdAt);
    return isEnded ? `finished after ${dur}` : `active for ${dur}`;
  }, [octopus.createdAt, isEnded]);

  const lastActive = useMemo(() => {
    if (!octopus.lastOutputAt) return null;
    return relativeTime(octopus.lastOutputAt);
  }, [octopus.lastOutputAt]);

  // Position card near click, clamped to viewport
  const cardW = 260;
  const cardH = 320;
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
            {octopus.unrestricted && (
              <span
                className="ml-auto rounded px-1 py-0.5 text-[10px] font-bold leading-none text-amber-300"
                style={{ backgroundColor: 'rgba(251, 191, 36, 0.15)' }}
                data-testid="unrestricted-badge"
              >
                unrestricted
              </span>
            )}
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
          </div>

          {/* Details */}
          <div className="space-y-1.5 text-xs text-slate-400">
            <div>
              Provider: <span className="text-slate-300">{octopus.provider}</span>
            </div>
            {octopus.model && (
              <div data-testid="profile-model">
                Model: <span className="text-slate-300">{shortenModel(octopus.model)}</span>
              </div>
            )}
            <div>
              Mode: <span className="text-slate-300">{octopus.mode}</span>
            </div>
            {octopus.ink && (
              <div>
                Ink: <span style={{ color }}>{octopus.ink}</span>
              </div>
            )}
            <div>
              Node: <span className="text-slate-300">{octopus.nodeName}</span>
            </div>
            <div data-testid="profile-workdir">
              Workdir: <span className="text-slate-300">{truncateWorkdir(octopus.workdir)}</span>
            </div>
            <div data-testid="profile-duration">
              <span className="text-slate-300">{duration}</span>
            </div>
            {lastActive && (
              <div data-testid="profile-last-active">
                Last active: <span className="text-slate-300">{lastActive}</span>
              </div>
            )}
            {octopus.interventionReason && (
              <div data-testid="profile-intervention" className="text-yellow-400">
                Intervention: {octopus.interventionReason}
              </div>
            )}
          </div>

          {/* Actions */}
          <div className="mt-4 flex gap-2">
            {onAttach && (
              <button
                onClick={() => onAttach(octopus.name)}
                className="rounded px-2.5 py-1 text-xs font-medium text-white hover:opacity-90"
                style={{ backgroundColor: '#2563eb' }}
                data-testid="attach-button"
              >
                Open Session
              </button>
            )}
            {onKill && KILLABLE_STATUSES.includes(octopus.status) && (
              <button
                onClick={() => onKill(octopus.name)}
                className="rounded px-2.5 py-1 text-xs font-medium text-white hover:opacity-90"
                style={{ backgroundColor: '#dc2626' }}
                data-testid="kill-button"
              >
                Kill
              </button>
            )}
            {onDelete && DELETABLE_STATUSES.includes(octopus.status) && (
              <button
                onClick={() => onDelete(octopus.name)}
                className="rounded px-2.5 py-1 text-xs font-medium text-white hover:opacity-90"
                style={{ backgroundColor: '#6b7280' }}
                data-testid="delete-button"
              >
                Delete
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
