# Pulpo Web UI

React 19 + Vite + Tailwind CSS v4 + shadcn/ui single-page application for the Pulpo dashboard.

## How It's Deployed

The web UI is built as a static SPA and embedded into the `pulpod` binary via `rust-embed`. When you run `pulpod`, the dashboard is served at `http://localhost:7433/`.

## Development

Requires **Node.js 22+** and a running `pulpod` instance on port 7433.

```bash
npm install
npm run dev       # starts dev server on port 5173, proxies /api to pulpod on 7433
```

Open `http://localhost:5173` in your browser. Hot-reload is enabled.

## Testing

```bash
npm test          # vitest + jsdom, runs all *.test.ts(x) files
```

## Building

```bash
npm run build     # outputs to build/ directory
```

The `build/` output is consumed by `make build` in the repo root, which embeds it into the `pulpod` binary.

## Project Structure

```
src/
├── index.css                          # Tailwind imports + dark theme CSS vars
├── main.tsx                           # Entry point
├── App.tsx                            # React Router setup
├── api/
│   ├── types.ts                       # Shared TypeScript interfaces
│   ├── client.ts                      # API fetch functions (20+)
│   └── connection.ts                  # testConnection, discoverPeers
├── hooks/
│   ├── use-connection.tsx             # Connection context (baseUrl, token, saved)
│   └── use-sse.tsx                    # SSE event stream + session state
├── lib/
│   ├── utils.ts                       # cn() helper, formatDuration
│   └── notifications.ts              # Desktop notification helpers
├── components/
│   ├── ui/                            # shadcn generated components
│   ├── layout/                        # Sidebar, header, app shell
│   ├── dashboard/                     # Status summary, node/session cards, new session
│   ├── session/                       # Chat view, terminal view (xterm.js)
│   ├── history/                       # Session filter, session list
│   ├── settings/                      # Node, guard, peer settings
│   └── connect/                       # Connect form, saved connections
└── pages/
    ├── dashboard.tsx                  # Real-time session dashboard
    ├── history.tsx                    # Session history with search/filter
    ├── settings.tsx                   # Node, guards, peers config
    └── connect.tsx                    # Connection screen (standalone)
```
