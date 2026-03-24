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

/** Format a status change into a human-readable toast label. */
export function formatStatusLabel(change: StatusChange): string {
  const label = change.to === 'ready' ? 'ready' : change.to === 'stopped' ? 'stopped' : 'resumed';
  const parts = [change.sessionName, label];

  if (change.errorStatus) {
    parts.push(`(${change.errorStatus})`);
  } else if (change.gitBranch) {
    const ins = change.gitInsertions ?? 0;
    const del = change.gitDeletions ?? 0;
    if (ins > 0 || del > 0) {
      const files = change.gitFilesChanged ?? 0;
      parts.push(`(+${ins}/-${del}, ${files} files on ${change.gitBranch})`);
    } else {
      parts.push(`on ${change.gitBranch}`);
    }
  }

  return parts.join(' ');
}

/**
 * Check for session status changes and trigger notifications.
 * Returns updated previousSessions array for the next check.
 */
export function processSessionChanges(
  previousSessions: Session[],
  currentSessions: Session[],
  toast: (msg: string) => void,
  notify: (change: StatusChange) => void,
): Session[] {
  if (previousSessions.length > 0) {
    const changes = detectStatusChanges(previousSessions, currentSessions);
    for (const change of changes) {
      toast(formatStatusLabel(change));
      notify(change);
    }
  }
  return [...currentSessions];
}

/** Request browser notification permission. Returns true if granted. */
export async function requestNotificationPermission(): Promise<boolean> {
  if (typeof Notification === 'undefined') return false;
  if (Notification.permission === 'granted') return true;

  const result = await Notification.requestPermission();
  return result === 'granted';
}

/** Build a concise enrichment suffix for notification body text. */
function enrichmentSuffix(change: StatusChange): string {
  const parts: string[] = [];

  if (change.errorStatus) {
    parts.push(`Error: ${change.errorStatus}`);
  }

  if (change.gitBranch) {
    const ins = change.gitInsertions ?? 0;
    const del = change.gitDeletions ?? 0;
    if (ins > 0 || del > 0) {
      const files = change.gitFilesChanged ?? 0;
      parts.push(`+${ins}/-${del} (${files} files) on ${change.gitBranch}`);
    } else {
      parts.push(`on branch ${change.gitBranch}`);
    }
  }

  return parts.length > 0 ? ` — ${parts.join(', ')}` : '';
}

/** Show a desktop notification for a status change. */
export function showDesktopNotification(change: StatusChange): void {
  if (typeof Notification === 'undefined' || Notification.permission !== 'granted') return;

  const { sessionName, to } = change;
  const suffix = enrichmentSuffix(change);

  let title: string;
  let body: string;

  if (to === 'ready') {
    title = `Session ready: ${sessionName}`;
    body = `${sessionName} is ready${suffix}`;
  } else if (to === 'stopped') {
    title = `Session stopped: ${sessionName}`;
    body = `${sessionName} has been stopped${suffix}`;
  } else {
    title = `Session resumed: ${sessionName}`;
    body = `${sessionName} is now active${suffix}`;
  }

  new Notification(title, { body });
}
