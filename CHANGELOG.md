# Changelog

## [0.0.8](https://github.com/darioblanco/pulpo/compare/v0.0.7...v0.0.8) (2026-03-08)


### Features

* **inks:** rename persona to ink across all layers ([7d474f8](https://github.com/darioblanco/pulpo/commit/7d474f8441bbd1940bd005df8ac8115e6540e68c))
* **peers:** scheme-aware peer addressing for Tailscale multi-node support ([02073ff](https://github.com/darioblanco/pulpo/commit/02073ffcc8d1159c45f04a5d41435d1c99610ab3))
* **session:** octopus-themed name generator for unnamed sessions ([7bd17a6](https://github.com/darioblanco/pulpo/commit/7bd17a6fb8572c5a4dc7c4b6c4945271d5f465b9))
* **tailscale:** auto-manage tailscale serve for HTTPS dashboard access ([7aa371c](https://github.com/darioblanco/pulpo/commit/7aa371c8571ac9f4e76fcdd03218a03af2270de0))
* **web:** terminal-window session cards + tmux mouse scrollback ([2130f8d](https://github.com/darioblanco/pulpo/commit/2130f8dd83149fe183489e02fb7a0421374c16b7))


### Bug Fixes

* **coverage:** align pre-commit threshold with cargo-llvm-cov 0.8+ behavior ([8961ff3](https://github.com/darioblanco/pulpo/commit/8961ff3ac1b48a581d212d08f9561c6f913adf6e))
* resolve attach session via API + restore 100% coverage ([f4dcfbe](https://github.com/darioblanco/pulpo/commit/f4dcfbe80f5951da912513d31ac64b35910eb1b2))
* **web:** update tailscale bind description + fix PeerEntry type ([c3ce927](https://github.com/darioblanco/pulpo/commit/c3ce927cfcfaa8b8a5d016df7c236e0c8cf014d7))

## [0.0.7](https://github.com/darioblanco/pulpo/compare/v0.0.6...v0.0.7) (2026-03-07)


### Features

* **cli:** show command shortcuts in help output ([bfb7432](https://github.com/darioblanco/pulpo/commit/bfb74324d505a4e38dd54efb602519418aca5dfb))
* **config:** simplify Tailscale setup — derive discovery from bind mode ([93d289b](https://github.com/darioblanco/pulpo/commit/93d289b157a8b1985aeea486465df66d93edc781))
* generic webhook notifications + settings UI improvements ([e678cef](https://github.com/darioblanco/pulpo/commit/e678cef9eefd66d65113a1e381002744509065cb))
* **web:** settings UI overhaul — full config API, cards, node/global sections ([181f237](https://github.com/darioblanco/pulpo/commit/181f2376160f3606b48d1d8f1f16526431df2278))


### Bug Fixes

* clippy + prettier issues, align pre-commit coverage with CI ([4b07682](https://github.com/darioblanco/pulpo/commit/4b076825ceebb00f410fa52069ce7bfbbd210c4f))
* **docker:** drop arm64 platform to avoid slow QEMU emulation ([eaf2dbf](https://github.com/darioblanco/pulpo/commit/eaf2dbfab40c61e9112c07df376c225dfd669fb8))
* **docs:** add sass dependency and repair SPEC links ([dad72f5](https://github.com/darioblanco/pulpo/commit/dad72f5c9fb240fa34bc9238a1093f202f19d60e))
* **docs:** set VuePress base path for /pulpo/ pages ([b78a321](https://github.com/darioblanco/pulpo/commit/b78a3217646ca6b26474ca002c7bdd1ea77e85b9))
* gate tailscale bind test behind cfg(coverage) ([ab8e95e](https://github.com/darioblanco/pulpo/commit/ab8e95ee9df89a723d122a54ff34f68a728511a0))
* restore 100% line coverage with Backend trait defaults ([6fa239b](https://github.com/darioblanco/pulpo/commit/6fa239bd977a513483f512da231725ca2e481fca))
* **ws:** pass raw session name to spawn_attach, not prefixed backend ID ([5d643da](https://github.com/darioblanco/pulpo/commit/5d643daf9b79e0656fe2ac4e1396f8a69d6a0759))

## [0.0.6](https://github.com/darioblanco/pulpo/compare/v0.0.5...v0.0.6) (2026-03-06)


### Bug Fixes

* **release:** skip verify on pulpod crate publish ([ff62a00](https://github.com/darioblanco/pulpo/commit/ff62a00633e46fa5f50c372c54eba455af9f523c))
* **release:** use single-quoted strings for Homebrew DSL in formula generator ([2ff1ce2](https://github.com/darioblanco/pulpo/commit/2ff1ce2faeac7d78e4f72901c65d06abb3297dd8))

## [0.0.5](https://github.com/darioblanco/pulpo/compare/v0.0.4...v0.0.5) (2026-03-06)


### Bug Fixes

* **release:** escape HOMEBREW_PREFIX in formula environment_variables ([ec89605](https://github.com/darioblanco/pulpo/commit/ec89605d524440f269561be7bb7155d7c5438855))

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
