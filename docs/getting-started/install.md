# Install

Choose the install path based on where you want the runtime to live:

- laptop or always-on Mac/Linux machine: Homebrew or source install
- server or team-managed box: service install after binary or source setup

Windows is not supported — sessions run in tmux, which native Windows doesn't
have. Use [WSL2](https://learn.microsoft.com/windows/wsl/install) and follow
the Linux instructions below inside your distribution.

If you are still deciding whether Pulpo fits your workflow, read
[Use Cases](/getting-started/use-cases) first.

## Homebrew (macOS/Linux)

```bash
brew install darioblanco/tap/pulpo
```

This installs:

- `pulpod` (daemon)
- `pulpo` (CLI)
- `tmux` (dependency via formula)

## From Source

Requirements:

- Rust 1.82+
- Node.js 22+
- tmux 3.2+ (macOS/Linux only)

**Ubuntu/Debian prerequisites:**

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev tmux
```

**Build and install:**

```bash
git clone https://github.com/darioblanco/pulpo.git
cd pulpo
make setup
make build
make install
```

## Start the Daemon

**macOS (Homebrew):** The daemon starts automatically via `brew services`. To start/stop manually:

```bash
brew services start pulpo    # auto-start on login
brew services stop pulpo
```

**Linux (systemd):** Install as a user service:

```bash
make service-install-linux   # enables and starts the service
systemctl --user status pulpo
```

Or run directly:

```bash
pulpod &   # background
```

To skip manual downloads, run the cross-platform install script before enabling the service (it also works as an updater):

```bash
curl -fsSL https://raw.githubusercontent.com/darioblanco/pulpo/main/scripts/install-pulpo.sh | bash
```

Set `BIN_DIR` or `TARGET` in your environment before running the script if you need a different install directory or target triple. Re-running the script downloads the latest release and overwrites the binaries, so it doubles as the update path.

## Verify

```bash
pulpo spawn hello -d -- echo "Pulpo is working!"
pulpo ls
pulpo logs hello
```

The web dashboard is at [http://localhost:7433](http://localhost:7433) (installable as a PWA on your phone).
