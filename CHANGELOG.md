# Changelog

## [0.0.19](https://github.com/darioblanco/pulpo/compare/v0.0.18...v0.0.19) (2026-03-12)


### Features

* add dedicated watchdog and notifications API endpoints ([0020179](https://github.com/darioblanco/pulpo/commit/0020179006469a3cdbe4878615b754063497c744))
* add machine-readable intervention reason codes ([7c87477](https://github.com/darioblanco/pulpo/commit/7c87477f602efe8bc4be043ca25820811241010e))
* add session_defaults config section ([d778c69](https://github.com/darioblanco/pulpo/commit/d778c69278fd7980d1b2c8dbbbc17aeb3e5ed167))
* **api:** populate memory_mb and gpu in peers endpoint ([5a8e18c](https://github.com/darioblanco/pulpo/commit/5a8e18ccbc9b1e41a1acfd016a29379663d3fe8c))
* **culture:** AGENTS.md compilation, scoped injection, and structured write-back ([d7672a9](https://github.com/darioblanco/pulpo/commit/d7672a9077cbf0e71210e46a97f2938e53b2ff56))
* **culture:** content validation and quality guidance for write-backs ([3835740](https://github.com/darioblanco/pulpo/commit/383574057abf10c39a784ed573e390d013414b0e))
* **culture:** cross-node sync with background pull loop and conflict resolution ([caba6f7](https://github.com/darioblanco/pulpo/commit/caba6f73fc05807269f483c694f81db0bab1f19c))
* **culture:** deduplication at harvest time ([5e2b66b](https://github.com/darioblanco/pulpo/commit/5e2b66baf009a9a943ebe55f109d0d98ccc333e0))
* **culture:** dynamic relevance decay with reference tracking ([abfc4b7](https://github.com/darioblanco/pulpo/commit/abfc4b7b76fbd6b530a5d72d4a8e39119243e07b))
* **culture:** exclude stale and superseded entries from AGENTS.md ([cd307bd](https://github.com/darioblanco/pulpo/commit/cd307bd3acd95fe8b482b58f69c40d7a01a0b2a7))
* **culture:** file browser API and UI for culture repo ([b5b5b59](https://github.com/darioblanco/pulpo/commit/b5b5b59f30c1172a37fd7fb4fa173202431fef70))
* **culture:** harvest agent write-backs, replace rule-based extraction ([e895aa8](https://github.com/darioblanco/pulpo/commit/e895aa85ca3e38255ab5310965ec3ceca15889c9))
* **culture:** lifecycle management with reference tracking and TTL decay ([250d7cf](https://github.com/darioblanco/pulpo/commit/250d7cff9da617fe5a979cab0473df634f424fd0))
* **culture:** optional YAML frontmatter in pending write-back files ([95b55fa](https://github.com/darioblanco/pulpo/commit/95b55fa35701809392bd2ceb3e202e33e3173b11))
* **discord:** forward culture SSE events to notification channel ([868a562](https://github.com/darioblanco/pulpo/commit/868a56227eeccad603ed41e90dd9f40ed5fcc0c5))
* **docker:** add Tailscale sidecar compose profile ([a5da0a8](https://github.com/darioblanco/pulpo/commit/a5da0a88124ea8e05a1a46528dcb05e32bacfdda))
* live watchdog config reload via watch channel ([3c397bc](https://github.com/darioblanco/pulpo/commit/3c397bc93574469d8287cbaf2d7829f21f7f2139))
* provider availability detection, compatibility matrix, and bare shell sessions ([33ef3c1](https://github.com/darioblanco/pulpo/commit/33ef3c1891db44b69e5c43c1c9748bc7a932206b))
* **session:** finished detection on agent exit marker ([5d4c1d2](https://github.com/darioblanco/pulpo/commit/5d4c1d2de95d6a82f3dea6af30bf9dc5b05149c3))
* **session:** finished TTL cleanup and resume semantics ([36ad150](https://github.com/darioblanco/pulpo/commit/36ad15037374f57b68bda928b5aec5de9fd27fdb))
* **session:** idle detection with Active ⇄ Idle state transitions ([68bf3d7](https://github.com/darioblanco/pulpo/commit/68bf3d7e5a237728db7b17e885a2d34e7ee8a285))
* **web:** real-time culture updates via SSE events ([a093e9d](https://github.com/darioblanco/pulpo/commit/a093e9df382a9c7cf70ad0f66aac64927ce0089c))


### Bug Fixes

* skip provider binary check in test builds ([503a9a4](https://github.com/darioblanco/pulpo/commit/503a9a4f3a789c30faa8a19959889678ef28c5bf))
* use explicit -b main for git init in tests and production ([e21b1a5](https://github.com/darioblanco/pulpo/commit/e21b1a5c91641863cccd13b02a95b8e08936e33e))

## [0.0.18](https://github.com/darioblanco/pulpo/compare/v0.0.17...v0.0.18) (2026-03-11)


### Features

* allow passing conversation_id at spawn to resume conversations ([0f2e6e1](https://github.com/darioblanco/pulpo/commit/0f2e6e1c97e1b08db7d70be3059643fda1139e58))


### Bug Fixes

* **ocean:** anchor seabed decorations to canvas bottom and scatter evenly ([4f0cc37](https://github.com/darioblanco/pulpo/commit/4f0cc375a1b36cc4aff50fb59c4161bcf804ca09))

## [0.0.17](https://github.com/darioblanco/pulpo/compare/v0.0.16...v0.0.17) (2026-03-11)


### Features

* **ocean:** animated fish schools, seabed decor, bigger labels, sharks ([f50af9f](https://github.com/darioblanco/pulpo/commit/f50af9f7065b01fb1f97103184107acf3808e8fb))

## [0.0.16](https://github.com/darioblanco/pulpo/compare/v0.0.15...v0.0.16) (2026-03-10)


### Features

* **ocean:** add kill and delete session actions to profile card ([ca887e3](https://github.com/darioblanco/pulpo/commit/ca887e3985acfc235a8f1356d1cfe48ec324ed98))
* **ocean:** ambient effects, hue variations, expand/collapse pools ([0d53d89](https://github.com/darioblanco/pulpo/commit/0d53d89aa7d9f6656779fef582b7dc883b079e44))
* **ocean:** new sprites, sea background, sunken ship landmark, badge positioning ([797111a](https://github.com/darioblanco/pulpo/commit/797111a174845e7381b2902846acbcc6d302c4fd))

## [0.0.15](https://github.com/darioblanco/pulpo/compare/v0.0.14...v0.0.15) (2026-03-10)


### Features

* **ocean:** tide pool grid, parallax backgrounds, enriched profile cards ([6c15bd1](https://github.com/darioblanco/pulpo/commit/6c15bd186ecb0fdd138d99dc9a204a34287294b9))


### Bug Fixes

* **daemon:** resolve agent binaries and wrap all sessions in bash for survival ([c3802eb](https://github.com/darioblanco/pulpo/commit/c3802ebd046d6fcd4e21771ca67782b584f60b44))
* **ocean:** replace dead octopus sprites with red X-eyed variant ([587678d](https://github.com/darioblanco/pulpo/commit/587678d1576be6f9b42665ce08fb33b76a6ccc2b))

## [0.0.14](https://github.com/darioblanco/pulpo/compare/v0.0.13...v0.0.14) (2026-03-10)


### Features

* **ocean:** replace SVG with pixel-art Canvas game engine ([22ed115](https://github.com/darioblanco/pulpo/commit/22ed115f200d3467efa4f7916f1f315452c506eb))


### Bug Fixes

* **daemon:** skip empty prompt arg in interactive mode to prevent immediate exit ([d200812](https://github.com/darioblanco/pulpo/commit/d200812745c20b4e9eabb5f52bde3aa3f493228c))
* prettier formatting for index.css and pre-commit hook cd leaking ([c2f4be3](https://github.com/darioblanco/pulpo/commit/c2f4be3eeb73f09f920aea0713533ac10ddfb454))

## [0.0.13](https://github.com/darioblanco/pulpo/compare/v0.0.12...v0.0.13) (2026-03-09)


### Features

* **cli:** add --worktree flag for opt-in git worktree isolation ([c9ec272](https://github.com/darioblanco/pulpo/commit/c9ec272c584f7f337b6a1e2db95ead540aecda2a))


### Bug Fixes

* **cli:** reject attach on stale/dead sessions with helpful message ([cf7e55f](https://github.com/darioblanco/pulpo/commit/cf7e55f1e0d7088c65ac51d2864ee2045c9cb116))

## [0.0.12](https://github.com/darioblanco/pulpo/compare/v0.0.11...v0.0.12) (2026-03-09)


### Bug Fixes

* only set worktree flag for providers that support it ([33b194b](https://github.com/darioblanco/pulpo/commit/33b194ba6ea1fe69d22e4ff2dd68a00867b70559))

## [0.0.11](https://github.com/darioblanco/pulpo/compare/v0.0.10...v0.0.11) (2026-03-09)


### Bug Fixes

* remove pulpo- prefix from tmux session names ([a38be0f](https://github.com/darioblanco/pulpo/commit/a38be0f78d56fa2f109fb97d37e82244f52a5971))

## [0.0.10](https://github.com/darioblanco/pulpo/compare/v0.0.9...v0.0.10) (2026-03-09)


### Bug Fixes

* **cli:** correct --name help text to say auto-generated ([f232208](https://github.com/darioblanco/pulpo/commit/f232208eb57b3c860f4a749ee39240735b27d757))
* **cli:** deserialize CreateSessionResponse wrapper from spawn API ([d63d820](https://github.com/darioblanco/pulpo/commit/d63d820bd0c7572af5b01c79fffde58f52b8a3ef))
* **web:** add proper padding to session input bar ([6b80325](https://github.com/darioblanco/pulpo/commit/6b8032540d9146369b6bfc915acb5804dd6c3bf2))

## [0.0.9](https://github.com/darioblanco/pulpo/compare/v0.0.8...v0.0.9) (2026-03-08)


### Features

* **guards:** replace 3-level presets with binary unrestricted toggle ([bb46c8c](https://github.com/darioblanco/pulpo/commit/bb46c8c59665dd4d0c87becab3264ede032f2ca9))
* **inks:** add model field to InkConfig for per-ink model selection ([3c4df80](https://github.com/darioblanco/pulpo/commit/3c4df80867e0a27af582eedd9cdef592aab4378b))
* **culture:** extract and store learnings from completed sessions ([334850b](https://github.com/darioblanco/pulpo/commit/334850b88a9a6cd342f036557b157bdcc6ce9589))
* **culture:** human CRUD API, CLI commands, and manual push ([428c5ae](https://github.com/darioblanco/pulpo/commit/428c5ae891947786d4d2d33384de517264b3d371))
* **culture:** inject context + write-back instructions at session spawn ([d083258](https://github.com/darioblanco/pulpo/commit/d0832586c23c9d7019aeeea122a8179d30c367ea))
* **culture:** replace SQLite storage with git-backed repository ([0ceaf18](https://github.com/darioblanco/pulpo/commit/0ceaf180d48e9318f93672e60a4e9f435b93c39e))
* **web:** culture browser page with CRUD and push-to-remote ([e38cf91](https://github.com/darioblanco/pulpo/commit/e38cf916d262bc48dc500412d10245b7470607b9))
* **web:** remove model dropdown from create session dialog ([36986a6](https://github.com/darioblanco/pulpo/commit/36986a6b7f9abf972a1b54f20310cfe8ec6e51e4))
* **web:** remove model dropdown from create session dialog ([9c30110](https://github.com/darioblanco/pulpo/commit/9c3011093985ac0a5dd28c1151e9b6f964198811))
* zero-arg spawn, culture markdown format, ocean visualization ([fd19ba9](https://github.com/darioblanco/pulpo/commit/fd19ba94410a93f539df151ee6f608b026238ab1))

## [0.0.8](https://github.com/darioblanco/pulpo/compare/v0.0.7...v0.0.8) (2026-03-08)


### Features

* **inks:** built-in ink presets + settings CRUD + session dialog selector ([5802638](https://github.com/darioblanco/pulpo/commit/5802638e5c431ab72b63157f4ca3a74229454119))
* **inks:** push-to-peers sync for ink presets ([e1a39c9](https://github.com/darioblanco/pulpo/commit/e1a39c9b0a8fee225b4369f38a5e78058767352d))
* **inks:** remove model from ink config + add provider-aware model selector ([097ab5e](https://github.com/darioblanco/pulpo/commit/097ab5edd463def0bb5382b17dcfc7ea742018e9))
* **inks:** rename persona to ink across all layers ([7d474f8](https://github.com/darioblanco/pulpo/commit/7d474f8441bbd1940bd005df8ac8115e6540e68c))
* **inks:** simplify to universal roles + fix MCP spawn_session ink bug ([6307563](https://github.com/darioblanco/pulpo/commit/63075638179369d35b64047f804bb8f17c9a9d8f))
* **peers:** scheme-aware peer addressing for Tailscale multi-node support ([02073ff](https://github.com/darioblanco/pulpo/commit/02073ffcc8d1159c45f04a5d41435d1c99610ab3))
* **providers:** add OpenCode provider with capability warnings ([c7937ec](https://github.com/darioblanco/pulpo/commit/c7937ec5d73547a68d15cac57b2b3079c64abef1))
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
