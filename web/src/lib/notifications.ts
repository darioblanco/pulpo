import type { Session } from '@/api/types';

/**
 * Trimmed to the fields the only consumer (`pages/dashboard.tsx`'s status toast)
 * actually reads. Previously carried `sessionId`, `from`, git diff/branch fields,
 * `errorStatus`, and a `prUrl` that was never populated by `detectStatusChanges`
 * — dead weight that suggested notification data (PR links, git diff stats) this
 * module never produced. Add a field back only once a real consumer needs it.
 */
export interface StatusChange {
  sessionName: string;
  to: string;
  /** `curr.status_reason` at the time of the transition — lets a consumer tell an
   * informational `done` (`exited`) apart from a forced one (`stopped`, an
   * intervention code, ...) since `working→done` now covers both of the old
   * `active→ready`/`active→stopped` transitions. */
  toReason?: string | null;
}

/**
 * Interesting transitions that warrant notification. Five-state model (ADR 0009):
 * `working→done` covers the old `active→ready` (clean exit) and `active→stopped`
 * (forced/explicit stop) transitions — merged since `ready`/`stopped` both became
 * `done`; distinguish them via `StatusChange.toReason` if needed.
 */
const INTERESTING_TRANSITIONS = new Set(['working→done', 'lost→working']);

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
        sessionName: curr.name,
        to: curr.status,
        toReason: curr.status_reason,
      });
    }
  }

  return changes;
}
