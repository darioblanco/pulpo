import type { Session } from '$lib/api';

export interface StatusChange {
  sessionId: string;
  sessionName: string;
  from: string;
  to: string;
}

/** Interesting transitions that warrant notification */
const INTERESTING_TRANSITIONS = new Set(['running→completed', 'running→dead', 'stale→running']);

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
      });
    }
  }

  return changes;
}

/** Request browser notification permission. Returns true if granted. */
export async function requestNotificationPermission(): Promise<boolean> {
  if (typeof Notification === 'undefined') return false;
  if (Notification.permission === 'granted') return true;

  const result = await Notification.requestPermission();
  return result === 'granted';
}

/** Show a desktop notification for a status change. */
export function showDesktopNotification(change: StatusChange): void {
  if (typeof Notification === 'undefined' || Notification.permission !== 'granted') return;

  const { sessionName, to } = change;

  let title: string;
  let body: string;

  if (to === 'completed') {
    title = `Session completed: ${sessionName}`;
    body = `${sessionName} finished successfully`;
  } else if (to === 'dead') {
    title = `Session died: ${sessionName}`;
    body = `${sessionName} has died`;
  } else {
    title = `Session resumed: ${sessionName}`;
    body = `${sessionName} is now running`;
  }

  new Notification(title, { body });
}
