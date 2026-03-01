# Pulpo Web UI

Svelte 5 + SvelteKit + Tailwind CSS v4 + Konsta UI v5 single-page application for the Pulpo dashboard.

## How It's Deployed

The web UI is built as a static SPA (`adapter-static`) and embedded into the `pulpod` binary via `rust-embed`. When you run `pulpod`, the dashboard is served at `http://localhost:7433/`.

## Development

Requires **Node.js 22+** and a running `pulpod` instance on port 7433.

```bash
npm install
npm run dev       # starts dev server on port 5173, proxies /api to pulpod on 7433
```

Open `http://localhost:5173` in your browser. Hot-reload is enabled.

## Testing

```bash
npm test          # vitest + jsdom, runs all *.test.ts files
```

## Building

```bash
npm run build     # outputs to build/ directory
```

The `build/` output is consumed by `make build` in the repo root, which embeds it into the `pulpod` binary.

## Project Structure

```
src/
├── app.css                    # Tailwind imports + dark theme vars
├── app.html                   # HTML template
├── routes/                    # SvelteKit pages
│   ├── +layout.svelte         # Konsta App wrapper (iOS theme, dark)
│   ├── +layout.ts             # ssr=false, prerender=true
│   ├── +page.svelte           # Dashboard (sessions, nodes, FAB)
│   ├── connect/+page.svelte   # Connection screen (mobile remote)
│   ├── history/+page.svelte   # Session history with search/filter
│   └── settings/+page.svelte  # Settings (node, guards, peers)
└── lib/
    ├── api.ts                 # API client (dynamic base URL)
    ├── connection.ts          # Connection helpers
    ├── notifications.ts       # Notification helpers
    ├── stores/                # Svelte 5 runes-based stores
    │   ├── connection.svelte.ts
    │   └── notifications.svelte.ts
    └── components/            # Reusable Svelte components (Konsta UI + Tailwind)
```
