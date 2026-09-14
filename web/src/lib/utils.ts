import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';
import type { Session } from '@/api/types';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatDuration(startIso: string, endIso?: string | null): string {
  const start = new Date(startIso).getTime();
  const end = endIso ? new Date(endIso).getTime() : Date.now();
  const seconds = Math.floor((end - start) / 1000);

  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
}

export function formatRelativeTime(dateString: string): string {
  const seconds = Math.floor((Date.now() - new Date(dateString).getTime()) / 1000);
  if (seconds < 10) return 'just now';
  if (seconds < 60) return `${seconds} seconds ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} minute${minutes !== 1 ? 's' : ''} ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hour${hours !== 1 ? 's' : ''} ago`;
  const days = Math.floor(hours / 24);
  return `${days} day${days !== 1 ? 's' : ''} ago`;
}

export function formatMemory(mb: number): string {
  if (mb >= 1024) {
    const gb = Math.round(mb / 1024);
    return `${gb} GB`;
  }
  return `${mb} MB`;
}

const NEEDS_INPUT_PREFIX = 'needs_input:';

/**
 * Split a `needs_input:<reason>` `status_reason` into its inner reason — mirrors
 * `pulpo_common::session::status_reason::needs_input_reason`. `undefined` for
 * anything else, including plain `"idle"` (turn finished, nothing pending) — callers
 * that want to distinguish "blocked on me" from "just idle" branch on this.
 */
export function needsInputReason(reason: string | null | undefined): string | undefined {
  return reason?.startsWith(NEEDS_INPUT_PREFIX)
    ? reason.slice(NEEDS_INPUT_PREFIX.length)
    : undefined;
}

/**
 * Color-dot class for a session's status (five-state model, ADR 0009). `done` is
 * reason-aware, mirroring the backend's `lifecycle_severity`: an `exited` reason is
 * informational (the old `ready` treatment); anything else — `stopped`, an
 * intervention code, or an absent/unrecognized reason — is warn-ish (the old
 * `stopped` treatment). The underlying CSS tokens (`--color-status-*`) keep their
 * pre-ADR-0009 names; only the status vocabulary mapped onto them changed.
 */
export function sessionStatusColor(session: Pick<Session, 'status' | 'status_reason'>): string {
  switch (session.status) {
    case 'starting':
      return 'bg-status-creating';
    case 'working':
      return 'bg-status-active';
    case 'waiting':
      return 'bg-status-idle';
    case 'lost':
      return 'bg-status-lost';
    case 'done':
      return session.status_reason === 'exited' ? 'bg-status-ready' : 'bg-status-stopped';
    default:
      return 'bg-muted';
  }
}

export function isTerminal(status: string): boolean {
  return status === 'done' || status === 'lost';
}

/**
 * Status label per ADR 0009 (must match the CLI's identical formatting exactly):
 * - `starting` / `working` / `lost` → the bare word, no parenthetical.
 * - `waiting` + `idle` → `"waiting (idle)"`; `waiting` + `needs_input:<x>` →
 *   `"waiting (needs input: <x>)"` (verbatim `<x>`); no reason → bare `"waiting"`.
 * - `done` + `exited` → `"done (exit N)"` when `exit_code` is a number, else
 *   `"done (exited)"`; `stopped` → `"done (stopped)"`; `idle_timeout` → `"done (idle
 *   timeout)"`; `budget_exceeded` → `"done (budget exceeded)"`; `memory_pressure` →
 *   `"done (memory pressure)"`; no reason → bare `"done"` (defensive fallback); any
 *   other/unrecognized reason → `"done (stopped)"` (generic forward-compat fallback).
 */
export function formatSessionStatus(
  session: Pick<Session, 'status' | 'status_reason' | 'exit_code'>,
): string {
  const reason = session.status_reason;
  if (session.status === 'waiting') {
    const needsInput = needsInputReason(reason);
    if (needsInput !== undefined) return `waiting (needs input: ${needsInput})`;
    return reason ? `waiting (${reason})` : 'waiting';
  }
  if (session.status === 'done') {
    if (reason == null) return 'done';
    switch (reason) {
      case 'exited':
        return typeof session.exit_code === 'number'
          ? `done (exit ${session.exit_code})`
          : 'done (exited)';
      case 'stopped':
        return 'done (stopped)';
      case 'idle_timeout':
        return 'done (idle timeout)';
      case 'budget_exceeded':
        return 'done (budget exceeded)';
      case 'memory_pressure':
        return 'done (memory pressure)';
      default:
        // Forward-compat: an unrecognized `done` reason still needs a label.
        return 'done (stopped)';
    }
  }
  return session.status;
}
