# Release and Distribution

## Distribution channels

- GitHub Releases (binary assets)
- Homebrew tap formulas

## Release automation intent

- `release-please` handles release PR/versioning flow.
- Tag-triggered workflows publish binaries and update distribution channels.

## Operational checks after each release

1. Verify release assets exist in GitHub Releases.
2. Verify Homebrew formulas point to the new release asset URLs.
3. Test `brew install darioblanco/tap/pulpo`.

## Upgrading `pulpod`

### Automatic backups before migrating

Every time `pulpod` starts against a database with pending migrations (i.e. an
existing, previously-used `state.db` — not a brand-new one), it copies
`state.db` to `state.db.pre-<version>` in the data dir *before* running the
migrations, where `<version>` is the `pulpod` version doing the migrating.
Migrations can be irreversible (for example, migration 0008 dropped the
`secrets` table and 0009 dropped `push_subscriptions`), so this gives you a
snapshot to fall back to even when upgrading across several releases at once.
A backup at the same version is overwritten on a later run, and only the 3
most recent `state.db.pre-*` files are kept — older ones are pruned
automatically. This is logged at `INFO`.

To restore a backup: stop `pulpod`, move the backup over `state.db` (and
remove any `state.db-wal`/`state.db-shm` files so SQLite doesn't try to
replay a mismatched write-ahead log), then start `pulpod` again.

### The daemon never crash-loops on a bad database

Before this behavior existed, an owner upgrading from 0.0.39 to 0.3.0 hit a
pre-migration database that `pulpod` refused to open ("unsupported legacy
database schema detected"), exited with status 1, and was restarted
repeatedly by launchd — the CLI only ever reported "pulpod did not start in
time," and the real cause was sitting in a log file nobody thought to check.

`pulpod` now treats a database it can't use — a corrupt/non-SQLite file, an
unsupported pre-migration schema, or a downgrade (a newer `pulpod` having
applied a migration this binary's embedded migrator has never heard of,
surfacing as sqlx's `VersionMissing`) — as recoverable rather than fatal:

1. The unusable file (and any `state.db-wal`/`state.db-shm` siblings) is
   renamed to `state.db.unusable-<UTC timestamp>` in the same data dir.
2. An `ERROR`-level log line records why and where it was moved.
3. A `daemon` event (`db_unusable` subtype, `critical` severity) is emitted on
   the same event bus session/intervention events use, so a configured
   `[[webhooks]]` endpoint is notified too.
4. A fresh, empty database is created and migrated in its place, and startup
   continues normally.

Startup is refused (the old crash-on-exit behavior) only if creating that
*fresh* database also fails — for example, the data dir itself isn't
writable. That's a real environment problem the operator needs to fix by
hand; there's no unusable file to recover from in that case.

**Inspecting a quarantined `state.db.unusable-*` file**: it's an ordinary
SQLite file (or, for the corrupt-file case, whatever bytes were actually
there) — nothing pulpo-specific reads it back. To look at it:

```bash
sqlite3 ~/.pulpo/state.db.unusable-20260914T120000Z ".tables"
sqlite3 ~/.pulpo/state.db.unusable-20260914T120000Z "SELECT * FROM sessions LIMIT 5;"
```

If `sqlite3` reports "file is not a database," the original file was already
corrupt (not a SQLite file at all) rather than an unsupported schema version —
there's nothing to recover from it. Once you've inspected or archived a
quarantined file, it's safe to delete; `pulpod` never reads it again.
