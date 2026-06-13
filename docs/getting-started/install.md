# Install

Choose the install path based on where you want the runtime to live:

- laptop or always-on Mac/Linux machine: Homebrew or source install
- Windows: via WSL2
- server or team-managed box: service install after binary or source setup

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

## Windows (via WSL2)

Sessions run on tmux, which is not available on native Windows — `pulpod` running on native Windows cannot create sessions locally (spawning returns an error). Run Pulpo inside [WSL2](https://learn.microsoft.com/windows/wsl/install) instead, where it runs as a normal Linux install.

Inside your WSL2 distribution, follow the Linux instructions above (Homebrew or [From Source](#from-source)) to install `pulpod`, `pulpo`, and `tmux`. Sessions you spawn there run under Linux/tmux.

The native Windows `pulpo.exe` client can still talk to a remote `pulpod` (for example one running in WSL2 or on another machine):

```powershell
# Point the CLI at a remote daemon and list its sessions
.\pulpo.exe --node <remote-host>:7433 ls
```

It cannot host sessions on native Windows — use it only as a client.

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
