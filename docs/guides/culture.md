# Culture Guide

Pulpo's culture system enables collective learning — agents write back what they learn, and future sessions benefit from that accumulated knowledge.

## How It Works

1. **Session starts** → Pulpo injects compiled culture context into the agent's instructions
2. **Agent works** → discovers patterns, fixes, or gotchas about the codebase
3. **Agent writes back** → creates `pending/<session>.md` files in the culture repo
4. **Session ends** → Pulpo harvests pending files: validates, deduplicates, commits
5. **Cross-node sync** → background git pull/push propagates learnings across nodes

## Culture Directory Structure

```
<data_dir>/culture/
├── culture/                    # Global scope
│   ├── AGENTS.md               # Compiled output (auto-generated)
│   └── <id>-<title>.md         # Individual entries
├── repos/
│   └── <repo-slug>/
│       ├── AGENTS.md
│       └── <id>-<title>.md
├── inks/
│   └── <ink-name>/
│       ├── AGENTS.md
│       └── <id>-<title>.md
└── pending/
    ├── .gitkeep
    └── <session-id>.md         # Agent write-back files
```

## AGENTS.md Format

Each culture entry is a markdown file with YAML frontmatter:

```markdown
---
id: "550e8400-e29b-41d4-a716-446655440000"
session_id: "660e8400-e29b-41d4-a716-446655440001"
kind: summary
scope_repo: "/Users/me/repos/my-api"
scope_ink: null
title: "Auth module uses PKCE flow for all OAuth providers"
tags:
  - claude
  - completed
relevance: 0.75
reference_count: 3
created_at: "2026-03-10T14:30:00Z"
last_referenced_at: "2026-03-12T09:15:00Z"
---

The auth module was refactored to use PKCE (Proof Key for Code Exchange) for all OAuth
providers, not just public clients. This affects token refresh logic — always include the
code_verifier when exchanging refresh tokens.
```

## Writing Good Culture Entries

Agents are instructed to write entries to `pending/<session-id>.md`. Good entries are:

- **Actionable** — describe what to do or avoid, not just what happened
- **Specific** — reference concrete files, functions, or patterns
- **Non-obvious** — capture knowledge that isn't self-evident from the code

### Validation Rules

Entries are validated on harvest:
- Title: 10–120 characters
- Body: 30+ characters
- Title must not equal body
- Body must not be only fenced code blocks (must include explanation)

### Optional Frontmatter

Agents can include YAML frontmatter in pending files for richer metadata:

```markdown
---
kind: failure
supersedes: "550e8400-e29b-41d4-a716-446655440000"
tags:
  - auth
  - security
---
# Token refresh requires code_verifier

When refreshing OAuth tokens, always include the code_verifier parameter...
```

Fields:
- `kind`: `summary` (default) or `failure`
- `supersedes`: ID of an entry this replaces
- `tags`: additional tags for filtering

If frontmatter is omitted, entries default to `kind: summary` with no extra tags.

## Relevance and Lifecycle

Each entry has a relevance score (0.0–1.0) that determines whether it's included in compiled output:

**Relevance formula**: `(0.8 - 0.1 × age_months + 0.05 × reference_count).clamp(0.0, 1.0)`

- **Age decay**: -0.1 per month, capped at 6 months of decay
- **Reference boost**: +0.05 per reference, capped at 4 references
- Entries below the stale threshold (based on `ttl_days`) are excluded from compilation

### Entry States

- **Active**: included in compiled AGENTS.md
- **Stale**: excluded from compilation (low relevance / old)
- **Superseded**: replaced by a newer entry, excluded from compilation

### Curation

Entries can be approved or rejected via the API or web UI:

```bash
# Via API
curl -X POST http://localhost:7433/api/v1/culture/<id>/approve \
  -d '{"approved": true}'
```

## Deduplication

When harvesting pending entries, Pulpo checks for existing entries with similar titles (case-insensitive substring match within the same scope). If a duplicate is found and the new entry is longer, the old entry is automatically superseded.

## Cross-Node Sync

When a `remote` is configured, culture syncs automatically:

1. **Push**: fire-and-forget after each commit (save, harvest)
2. **Pull**: background loop every `sync_interval_secs` (default: 300s)
3. **Conflicts**: rebase-first strategy. On conflict, abort rebase and merge with local-wins resolution.
4. **Scope filtering**: when `sync_scopes` is set, only files in matching directories are kept after pull

Check sync status:

```bash
curl http://localhost:7433/api/v1/culture/sync
```

## Configuration

```toml
[culture]
remote = "git@github.com:yourorg/pulpo-culture.git"
inject = true                    # Inject context into sessions (default: true)
ttl_days = 90                    # Days before stale (default: 90)
curator = "claude"               # Provider for curation (optional)
sync_interval_secs = 300         # Sync interval (default: 300)
sync_scopes = ["culture"]        # Limit sync to these scopes (optional)
```
