# 0005. The config file is the source of truth; no config-editing API

- **Status:** Accepted
- **Date:** 2026-09-13 (PR [#117](https://github.com/darioblanco/pulpo/pull/117))

## Context

`pulpod` exposed `PUT /api/v1/config`, `/watchdog`, and `/notifications`, backing a web
UI settings tabbar (Node, Watchdog, Notifications) with "restart required" detection
and a config-file rewrite performed on every settings save. This machinery existed to
let the web UI edit configuration without touching `~/.pulpo/config.toml` directly, but
it meant the daemon owned two sources of truth for its own configuration (the file, and
whatever the API had last written back into it) that had to be kept in sync, and a
retired config key could get silently rewritten out of the file the next time any
unrelated save happened to fire.

## Decision

We will make `~/.pulpo/config.toml` the **sole** source of truth. Remove the three
`PUT` handlers and their request/response types
(`UpdateConfigRequest`/`UpdateConfigResponse`/`UpdateWatchdogRequest`/
`UpdateNotificationsRequest`/`WebhookEndpointUpdateRequest`), the "restart required"
detection, and the config-file rewrite-on-save. `GET /api/v1/config`, `/watchdog`, and
`/notifications` remain as **read-only** views of the effective config. The web UI's
settings page becomes a single read-only "Configuration" view (a formatted list of
effective config, with a hint to edit the file and restart). To change anything, an
operator edits `config.toml` and restarts `pulpod`.

A same-month follow-up (PR #118) removed the watchdog's runtime-config hot-reload
channel once this decision left it with no writer: `PUT /api/v1/watchdog` had been its
only sender, so `AppState.watchdog_config_tx`, `run_watchdog_loop`'s `config_rx`
parameter, and `refresh_watchdog_ticker` were all dead code afterward.

## Consequences

- A retired config key (e.g. `watchdog.adopt_tmux`) is now ignored forever rather than
  opportunistically dropped the next time a save happened to fire — simpler and more
  predictable, at the cost of old files accumulating dead keys until an operator edits
  them out by hand.
- Changing `[watchdog]` (or any other section) takes effect on the next `pulpod`
  restart, not live — consistent with every other config section, and with how a
  single-operator, self-hosted daemon is actually run.
- The web settings tabbar UI, `components/settings/{node,watchdog,notifications}-
  settings.tsx`, `form-field.tsx`, and the now-unused `components/ui/tabs.tsx` were
  deleted along with their tests.
- The PWA install path (`web/src/sw.ts`, `vite-plugin-pwa`, the web manifest, and the
  `apple-mobile-web-app-*`/`apple-touch-icon` assets) was removed in the same PR — an
  installable icon wasn't worth a service worker once the settings UI it was bundled
  with went read-only. The web UI is now a plain responsive page, reached by
  bookmarking it rather than installing it.
- This is a policy other config-surface decisions can point back to: no future feature
  should reintroduce a daemon-writes-its-own-config-file path without revisiting this
  ADR first.
