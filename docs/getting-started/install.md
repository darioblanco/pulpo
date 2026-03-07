# Install

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
- tmux 3.2+

```bash
git clone https://github.com/darioblanco/pulpo.git
cd pulpo
make setup
make build
make install
```

## Verify

```bash
pulpod --version
pulpo --version
tmux -V
```
