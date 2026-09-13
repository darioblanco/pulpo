import type { Session } from '@/api/types';

export interface StatusChange {
  sessionId: string;
  sessionName: string;
  from: string;
  to: string;
  gitBranch?: string | null;
  gitInsertions?: number | null;
  gitDeletions?: number | null;
  gitFilesChanged?: number | null;
  prUrl?: string | null;
  errorStatus?: string | null;
}

/** Interesting transitions that warrant notification */
const INTERESTING_TRANSITIONS = new Set(['active→ready', 'active→stopped', 'lost→active']);

/**
 * Compare previous and current session lists to detect interesting status changes.
 * Only reports transitions in INTERESTING_TRANSITIONS to avoid noise.
 */
export function detectStatusChanges(previous: Session[], current: Session[]): StatusChange[] {
  const prevMap = new Map(previous.map((s) => [s.id, s]));
  const changes: StatusChange[] = [];

  for (const curr of current) {
    const prev = prevMap.get(curr.id);
    if (!prev || prev.status === curr.status) continue;

    const transition = `${prev.status}→${curr.status}`;
    if (INTERESTING_TRANSITIONS.has(transition)) {
      changes.push({
        sessionId: curr.id,
        sessionName: curr.name,
        from: prev.status,
        to: curr.status,
        gitBranch: curr.git_branch,
        gitInsertions: curr.git_insertions,
        gitDeletions: curr.git_deletions,
        gitFilesChanged: curr.git_files_changed,
        errorStatus: curr.metadata?.error_status,
      });
    }
  }

  return changes;
}
