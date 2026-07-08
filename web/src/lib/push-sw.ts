/**
 * Pure, unit-testable pieces of the service worker's Web Push handling.
 *
 * `src/sw.ts` is excluded from coverage (it only runs in a real service worker
 * context), so the actual decision logic — what a notification should look
 * like, and what happens when its action button is tapped — lives here where
 * it can be exercised with plain Vitest, mirroring the coverage-split pattern
 * used on the Rust side (`notifications::web_push::build_payload` is pure and
 * tested; only the real HTTP send is `#[cfg(not(coverage))]`).
 */

/** One action button on a notification (a minimal `NotificationAction` — not
 * declared in this project's `lib.dom`/`lib.webworker` TypeScript libs). */
export interface PulpoNotificationAction {
  action: string;
  title: string;
}

/** `NotificationOptions` extended with the `actions` field the Notifications
 * API supports but this project's TypeScript lib definitions don't include. */
export interface PulpoNotificationOptions extends NotificationOptions {
  actions?: PulpoNotificationAction[];
}

/** The JSON body of a Web Push message sent by pulpod (see
 * `notifications::web_push::build_payload` in the Rust daemon). */
export interface PushPayload {
  title?: string;
  body?: string;
  url?: string;
  session_id?: string;
  /** Present only on `usage_alert` events — the actionable "Stop session" capability. */
  action?: {
    token: string;
    label: string;
  };
}

/** The `action` value used for the "Stop session" notification button, and the
 * value posted back to `POST /api/v1/push/action` — must match the Rust
 * daemon's `notifications::action_token::STOP_ACTION`. */
export const STOP_ACTION = 'stop';

/** Tag used for the follow-up "stopped" / "failed to stop" confirmation notification. */
const ACTION_RESULT_TAG = 'pulpo-action-result';

/** Build the `showNotification(title, options)` arguments for an incoming push payload. */
export function buildNotificationOptions(payload: PushPayload): {
  title: string;
  options: PulpoNotificationOptions;
} {
  const title = payload.title ?? 'Pulpo';
  const options: PulpoNotificationOptions = {
    body: payload.body ?? '',
    icon: '/icons/icon-192x192.png',
    badge: '/icons/icon-192x192.png',
    data: { url: payload.url ?? '/', actionToken: payload.action?.token },
    tag: payload.session_id ? `pulpo-session-${payload.session_id}` : 'pulpo-notification',
  };

  if (payload.action) {
    options.actions = [{ action: STOP_ACTION, title: payload.action.label || 'Stop session' }];
  }

  return { title, options };
}

export interface StopActionResult {
  success: boolean;
  sessionName?: string;
}

/**
 * POST the action token to the daemon's unauthenticated `/api/v1/push/action`
 * endpoint. `scopeUrl` is the service worker's registration scope (pulpo's web
 * UI and API are always same-origin, so no separate base-URL needs to travel
 * in the push payload). Never throws — network/parse failures resolve to
 * `{ success: false }` so the caller can always show a confirmation notification.
 */
export async function postStopAction(
  scopeUrl: string,
  token: string,
  fetchImpl: typeof fetch = fetch,
): Promise<StopActionResult> {
  try {
    const res = await fetchImpl(new URL('/api/v1/push/action', scopeUrl).toString(), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token }),
    });
    if (!res.ok) return { success: false };

    const data = (await res.json().catch(() => ({}))) as { session_name?: string };
    return { success: true, sessionName: data.session_name };
  } catch {
    return { success: false };
  }
}

/** Build the confirmation notification shown after a stop action completes. */
export function buildStopResultNotification(result: StopActionResult): {
  title: string;
  options: NotificationOptions;
} {
  if (result.success) {
    return {
      title: 'Session stopped',
      options: {
        body: result.sessionName ? `${result.sessionName} was stopped.` : 'Session was stopped.',
        icon: '/icons/icon-192x192.png',
        tag: ACTION_RESULT_TAG,
      },
    };
  }

  return {
    title: 'Stop failed',
    options: {
      body: 'Could not stop the session — it may already be gone, or the link expired.',
      icon: '/icons/icon-192x192.png',
      tag: ACTION_RESULT_TAG,
    },
  };
}
