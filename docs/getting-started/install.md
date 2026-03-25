# Install

## Homebrew (macOS/Linux)

```bash
brew install darioblanco/tap/pulpo
```

This installs:

- `pulpod` (daemon)
- `pulpo` (CLI)
- `tmux` (dependency via formula)

## Windows

Download `pulpod.exe` and `pulpo.exe` from [GitHub Releases](https://github.com/darioblanco/pulpo/releases).

Requirements:

- [Docker Desktop](https://docs.docker.com/desktop/install/windows-install/) (for `--runtime docker` sessions)

On Windows, all sessions use Docker containers (`--runtime docker`). tmux is not available on Windows, so sessions without `--runtime docker` will show an error directing you to use Docker.

```powershell
# Start daemon
.\pulpod.exe

# Spawn a Docker runtime session
.\pulpo.exe spawn my-task --runtime docker -- claude -p "Fix the bug"

# Open web UI
start http://localhost:7433
```

## From Source

Requirements:

- Rust 1.82+
- Node.js 22+
- tmux 3.2+ (macOS/Linux only)

```bash
git clone https://github.com/darioblanco/pulpo.git
cd pulpo
make setup
make build
make install
```

## Verify

```bash
pulpo spawn hello -d -- echo "Pulpo is working!"
pulpo ls
pulpo logs hello
```

The daemon starts automatically when you run your first `pulpo` command. The web dashboard is at [http://localhost:7433](http://localhost:7433) (installable as a PWA on your phone).
