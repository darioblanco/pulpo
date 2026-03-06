# Changelog

## [0.0.4](https://github.com/darioblanco/pulpo/compare/v0.0.3...v0.0.4) (2026-03-06)


### Bug Fixes

* **ci:** trigger release/docker on pulpo-v tags ([002b350](https://github.com/darioblanco/pulpo/commit/002b350eef2cd746fdf72bccb64ca610d41c02ae))
* **release:** enable GitHub release creation in release-please ([f6d5a48](https://github.com/darioblanco/pulpo/commit/f6d5a488f32b7ff03662c8557b230ecea5616db3))
* **release:** use v-tags and harden homebrew sync ([7de5a19](https://github.com/darioblanco/pulpo/commit/7de5a19de79b17f3f5bf8d6f7f6b56efe622e145))
* **service:** add PATH to launchd environment for tmux discovery ([fbc7317](https://github.com/darioblanco/pulpo/commit/fbc7317716934fc996fa19d58acb18e9d4e4dcfc))

## [0.0.3](https://github.com/darioblanco/pulpo/compare/pulpo-v0.0.2...pulpo-v0.0.3) (2026-03-06)


### Features

* **discovery:** add Tailscale and seed peer discovery methods ([8be6929](https://github.com/darioblanco/pulpo/commit/8be692943198975c2eea24fbb7b57872823ef68a))
* **docker:** add Discord bot Dockerfile and compose file ([acd1065](https://github.com/darioblanco/pulpo/commit/acd1065610981581e4e360044b1ba04f245eeb37))


### Bug Fixes

* **ci:** pin pulpo-agents base image by digest ([d511dee](https://github.com/darioblanco/pulpo/commit/d511dee98fa11cc8f8397945f2a0f3b71866a9dc))
* **ci:** set deterministic BASE_IMAGE for pulpo-agents build ([d07ff0e](https://github.com/darioblanco/pulpo/commit/d07ff0ee51f24c5d2302a90d8b3666ccad992e46))
* **ci:** use RELEASE_PLEASE_TOKEN and add discord image sha tag ([076978e](https://github.com/darioblanco/pulpo/commit/076978edab2ac8836a1e1f2f22d3efae175ba75e))
* **clippy:** replace needless collect in tailscale test ([dd4fe3d](https://github.com/darioblanco/pulpo/commit/dd4fe3dcd848ed89c9cda0ac657a21ca00ee5bbf))
* **release:** align release-please with cargo-dist assets ([d146bc8](https://github.com/darioblanco/pulpo/commit/d146bc81ce9bea28c48c51e84c8dfbadc9da3ed9))
* **release:** auto-sync pulpo homebrew formula on tag ([264c3c2](https://github.com/darioblanco/pulpo/commit/264c3c2559b07bb5cdea9078db6f886acca3cac9))

## [0.0.2](https://github.com/darioblanco/pulpo/compare/pulpo-v0.0.1...pulpo-v0.0.2) (2026-03-06)


### Bug Fixes

* **ci:** bootstrap release-please with current SHA ([486889f](https://github.com/darioblanco/pulpo/commit/486889f3d5a35d762ffb6b9ef2842cc0c295bf5b))
* **ci:** switch release-please to simple type to avoid Rust workspace issues ([c1e4f87](https://github.com/darioblanco/pulpo/commit/c1e4f87c983919baefbc9a27428c95f23849c38a))
