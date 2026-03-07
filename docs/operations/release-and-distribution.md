# Release and Distribution

## Distribution channels

- GitHub Releases (binary assets)
- Docker Hub images
- Homebrew tap formulas

## Release automation intent

- `release-please` handles release PR/versioning flow.
- Tag-triggered workflows publish binaries and update distribution channels.

## Operational checks after each release

1. Verify release assets exist in GitHub Releases.
2. Verify Docker workflows completed and images are pushed.
3. Verify Homebrew formulas point to the new release asset URLs.
4. Test `brew install darioblanco/tap/pulpo`.
