#!/usr/bin/env bash
# Scheduled sessions — run agents on a cron schedule.
#
# Schedules are DB-backed rows in pulpod's own SQLite store, fired by a 60s
# scheduler loop inside the daemon — no crontab entries are written anywhere.
# Useful for nightly code reviews, periodic security scans, etc.
set -euo pipefail

URL="${URL:-localhost:7433}"

# Install a nightly code review at 3 AM
pulpo --url "${URL}" schedule install nightly-review \
  "0 3 * * *" \
  --workdir ~/repos/my-api \
  -- claude -p "Review recent commits and flag any issues"

# Install a weekly security scan on Sundays at midnight
pulpo --url "${URL}" schedule install weekly-scan \
  "0 0 * * 0" \
  --workdir ~/repos/my-api \
  -- claude -p "Run a thorough security audit"

# List installed schedules
pulpo --url "${URL}" schedule list

# Pause a schedule (flips its `enabled` flag off — the scheduler loop skips it)
pulpo --url "${URL}" schedule pause nightly-review

# Resume a paused schedule
pulpo --url "${URL}" schedule resume nightly-review

# Remove a schedule
# pulpo --url "${URL}" schedule remove weekly-scan
