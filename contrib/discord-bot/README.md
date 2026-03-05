# Pulpo Discord Bot

Discord bot for controlling pulpod sessions — 7 slash commands with autocomplete and real-time SSE notifications.

## Prerequisites

- Node.js 20+
- A running `pulpod` instance
- A Discord bot token ([Developer Portal](https://discord.com/developers/applications))

## Quick Start

1. **Create bot** — Go to [discord.com/developers/applications](https://discord.com/developers/applications), create an app, go to **Bot** tab, copy the token. Under **OAuth2 > URL Generator**, select scopes `bot` + `applications.commands`, permissions `Send Messages` + `Embed Links`, then invite with the generated URL.

2. **Configure env** — Copy `.env.example` to `.env` and fill in:

   | Variable | Required | Description |
   |----------|----------|-------------|
   | `DISCORD_TOKEN` | Yes | Bot token from Developer Portal |
   | `PULPOD_URL` | No | Pulpod URL (default: `http://localhost:7433`) |
   | `PULPOD_TOKEN` | No | Auth token (required if pulpod uses `public` bind mode) |
   | `DISCORD_NOTIFICATION_CHANNEL_ID` | No | Channel for SSE notifications (fallback: spawn channel) |

3. **Install** — `npm install`

4. **Run** — `npm run dev` (development) or `npm run build && npm start` (production)

## Commands

All `session` options support **autocomplete** — type a few characters and matching sessions appear.

| Command | Description | Options |
|---------|-------------|---------|
| `/spawn` | Spawn a new agent session | `workdir` (required), `prompt` (required), `persona`, `model`, `name` |
| `/status` | Show session status | `session` (optional, omit for all) |
| `/logs` | Show recent session output | `session` (required), `lines` (1-500, default 50) |
| `/kill` | Kill a running session | `session` (required) |
| `/resume` | Resume a stale session after reboot | `session` (required) |
| `/personas` | List available persona configurations | — |
| `/input` | Send text input to a running session | `session` (required), `text` (required) |

## SSE Notifications

The bot connects to `GET /api/v1/events` and posts session lifecycle events (running, completed, dead, etc.) as Discord embeds. Events route to:

1. The channel set in `DISCORD_NOTIFICATION_CHANNEL_ID`, or
2. The channel where `/spawn` was used (stored in session metadata)

## Development

```bash
npm test           # Run tests (vitest)
npm run lint       # Type-check (tsc --noEmit)
npm run fmt        # Format (prettier --write)
npm run fmt:check  # Check formatting
npm run build      # Compile to dist/
```

## Architecture

```
src/
├── index.ts              # Entry: client init, command registration, autocomplete
├── config.ts             # Env-based config (BotConfig)
├── api/
│   ├── pulpod.ts         # HTTP client (PulpodClient)
│   └── pulpod.test.ts    # API client tests
├── commands/
│   ├── spawn.ts          # /spawn — create a new session
│   ├── status.ts         # /status — show session(s)
│   ├── logs.ts           # /logs — recent output
│   ├── kill.ts           # /kill — terminate session
│   ├── resume.ts         # /resume — resume stale session
│   ├── personas.ts       # /personas — list personas
│   └── input.ts          # /input — send text to session
├── listeners/
│   └── sse.ts            # EventSource -> Discord channel messages
└── formatters/
    ├── embed.ts          # Discord embed builders
    └── embed.test.ts     # Embed formatter tests
```
