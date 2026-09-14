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

### The daemon never crash-loops on a bad database — but only quarantines a bad one

Before this behavior existed, an owner upgrading from 0.0.39 to 0.3.0 hit a
pre-migration database that `pulpod` refused to open ("unsupported legacy
database schema detected"), exited with status 1, and was restarted
repeatedly by launchd — the CLI only ever reported "pulpod did not start in
time," and the real cause was sitting in a log file nobody thought to check.

`pulpod` treats a database it can't use as recoverable rather than fatal, but
only for failures that mean the *file itself* is corrupt or on a schema this
binary can't read — quarantining (renaming it away and starting fresh) is
only ever safe when the file is actually the problem:

- A corrupt/non-SQLite file (SQLite reports `"file is not a database"` or
  `"...malformed"`).
- An unsupported pre-migration schema (the pre-0.1.0 legacy-schema check).
- A downgrade: a newer `pulpod` applied a migration (or changed one) this
  binary's embedded migrator has never heard of, surfacing as sqlx's
  `VersionMissing`/`VersionMismatch`.

For any of those:

1. The unusable file (and any `state.db-wal`/`state.db-shm` siblings) is
   renamed to `state.db.unusable-<UTC timestamp>` in the same data dir.
2. An `ERROR`-level log line records why and where it was moved.
3. A `daemon` event (`db_unusable` subtype, `critical` severity) is emitted on
   the same event bus session/intervention events use, so a configured
   `[[webhooks]]` endpoint is notified too.
4. A fresh, empty database is created and migrated in its place, and startup
   continues normally.

**Every other failure refuses to start instead — without touching the file at
all.** A transient error (`"database is locked"`, a plain I/O error, a failed
pre-migration backup copy) says nothing about the database's own integrity:
quarantining on it risks renaming a perfectly healthy database out from under
a process that still holds a valid handle to it. `pulpod` logs the failure at
`ERROR` and exits with status 1 instead — under launchd/systemd this means a
restart is attempted (see the single-instance lock below for the specific
case this matters most for), which is the right response when the problem is
"something else has this file busy right now," not "this file is garbage."

Startup is also refused if creating a *fresh* database fails after a
legitimate quarantine — for example, the data dir itself isn't writable.
That's a real environment problem the operator needs to fix by hand; there's
no unusable file left to recover from in that case.

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

### Single-instance lock

`pulpod` acquires an exclusive advisory lock (`flock(2)`) on
`{data_dir}/pulpod.lock` at startup, *before* it ever opens `state.db` — and
holds it for as long as the process runs. This exists because opening (and,
on an unusable-database error, quarantining) the database happens well before
the HTTP port is bound, so without the lock a second `pulpod` accidentally
started against the same data directory could race the first one to open the
database and, worse, rename a database the first instance still has open and
considers perfectly healthy right out from under it.

If the lock is already held, `pulpod` logs `ERROR` — `another pulpod is
running (pid <pid>) against data directory <dir> — refusing to start` — and
exits with status 1 **without touching `state.db` at all**, not even to open
it. The recorded PID is best-effort (read back from the lock file the other
process wrote its own PID into) and purely informational.

The lock needs no manual cleanup: it's tied to the process's open file
descriptor, so the OS releases it automatically the moment the holding
`pulpod` exits — cleanly, on a crash, or via `SIGKILL`. There is never a
stale `pulpod.lock` to delete by hand (unlike a bare PID file); if you see the
"another pulpod is running" error and are sure no `pulpod` is actually
running against that data directory, look for a zombie process holding an
open file descriptor into it rather than deleting the lock file.

### Exit codes

`pulpod` exits **0** only on a clean, intentional shutdown (`SIGTERM`/
`SIGINT`, or the streaming-connection grace period simply elapsing). Every
startup failure — a config error, the single-instance lock already held, an
unrecoverable database error, a port already in use — exits **1**. Under
launchd/systemd this triggers a restart, which is the correct behavior for
every one of those except a config error (fix the config and the next restart
succeeds) — check the log (`journalctl`/`~/.pulpo/logs/`) for the `ERROR` line
explaining which case it was.
