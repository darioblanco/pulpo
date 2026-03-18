#!/usr/bin/env bash
# Scheduled sessions — run agents on a cron schedule.
#
# Pulpo manages crontab entries that spawn sessions automatically.
# Useful for nightly code reviews, periodic security scans, etc.
set -euo pipefail

NODE="${NODE:-localhost:7433}"

# Install a nightly code review at 3 AM
pulpo --node "${NODE}" schedule install nightly-review \
  "0 3 * * *" \
  --workdir ~/repos/my-api \
  -- claude -p "Review recent commits and flag any issues"

# Install a weekly security scan on Sundays at midnight
pulpo --node "${NODE}" schedule install weekly-scan \
  "0 0 * * 0" \
  --workdir ~/repos/my-api \
  -- claude -p "Run a thorough security audit"

# List installed schedules
pulpo --node "${NODE}" schedule list

# Pause a schedule (comments out the crontab line)
pulpo --node "${NODE}" schedule pause nightly-review

# Resume a paused schedule
pulpo --node "${NODE}" schedule resume nightly-review

# Remove a schedule
# pulpo --node "${NODE}" schedule remove weekly-scan
