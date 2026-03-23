# Changelog

## [0.0.33](https://github.com/darioblanco/pulpo/compare/v0.0.32...v0.0.33) (2026-03-23)


### Features

* rename kill→stop, fix worktree paths, fix PATH resolution for service mode ([7443c11](https://github.com/darioblanco/pulpo/commit/7443c1139bdb5657d016dd9389412a92fec774d7))

## [0.0.32](https://github.com/darioblanco/pulpo/compare/v0.0.31...v0.0.32) (2026-03-22)


### Features

* mobile PWA session orchestration and connectivity fixes ([a71c971](https://github.com/darioblanco/pulpo/commit/a71c971c627cb0c0942cbb70a9e98cb34d0a893f))


### Bug Fixes

* add bottom tab bar for mobile PWA navigation ([833d071](https://github.com/darioblanco/pulpo/commit/833d071939c923593e10c25f54bfdaf25ca2e561))
* hide sidebar trigger on mobile (bottom tab bar handles navigation) ([3038cf6](https://github.com/darioblanco/pulpo/commit/3038cf6060cc047bbfb6fb96d7411f1b312119cd))
* use $SHELL instead of hardcoded bash for wrap_command ([5cfd1a9](https://github.com/darioblanco/pulpo/commit/5cfd1a97e6adaeb70be7bbb59d55b2d2338a0e9a))

## [0.0.31](https://github.com/darioblanco/pulpo/compare/v0.0.30...v0.0.31) (2026-03-22)


### Features

* cleanup command + multi-name kill/delete ([b2efc60](https://github.com/darioblanco/pulpo/commit/b2efc60c1c97dd1ea0a837f2021b0a8f85209dd0))

## [0.0.30](https://github.com/darioblanco/pulpo/compare/v0.0.29...v0.0.30) (2026-03-21)


### Bug Fixes

* liveness check uses session ID instead of name ([fa632e7](https://github.com/darioblanco/pulpo/commit/fa632e7fbe1f4cf24881f114037918c5a4c4d6c8))
* skip liveness check for shell sessions (no explicit command) ([801b629](https://github.com/darioblanco/pulpo/commit/801b629ea2f2f5c4d70d385269e8ea67baece4ea))

## [0.0.29](https://github.com/darioblanco/pulpo/compare/v0.0.28...v0.0.29) (2026-03-21)


### Bug Fixes

* actually wire vendored OpenSSL into pulpod crate ([13460db](https://github.com/darioblanco/pulpo/commit/13460dbbf686a3407f90f85ceb341f2cc87febc9))

## [0.0.28](https://github.com/darioblanco/pulpo/compare/v0.0.27...v0.0.28) (2026-03-21)


### Features

* account/plan detection and rate limit warnings ([042dbb9](https://github.com/darioblanco/pulpo/commit/042dbb9a7d70e091ac0d7637f2945f840a17f4dd))


### Bug Fixes

* vendor OpenSSL for Windows builds ([a7d6dd6](https://github.com/darioblanco/pulpo/commit/a7d6dd69aaa70c1bd096ac3b98518425c2509de7))

## [0.0.27](https://github.com/darioblanco/pulpo/compare/v0.0.26...v0.0.27) (2026-03-21)


### Features

* detect PR URLs and branch names from agent output ([f2bd9ca](https://github.com/darioblanco/pulpo/commit/f2bd9cae93e62888b2c2f4139095b13d0beadff0))
* Docker volume mounts for agent auth + macOS Keychain extraction ([73cb2bc](https://github.com/darioblanco/pulpo/commit/73cb2bc71918bcaf477248361e08864fb3e86634))
* inks support secrets and runtime fields ([1deb3d6](https://github.com/darioblanco/pulpo/commit/1deb3d629e2950501b41e1091464a90ad98e65bd))
* schedule dashboard with create/edit dialog and cron utilities ([6fc952b](https://github.com/darioblanco/pulpo/commit/6fc952b60afd05092f90187cf4aff863fdba82d3))
* schedule run history — API endpoint and expandable UI panel ([172582b](https://github.com/darioblanco/pulpo/commit/172582b458903ef16ff1f66ba9be6ec845515a29))
* secret env override and session injection via --secret flag ([55e9001](https://github.com/darioblanco/pulpo/commit/55e9001ad1c43de79ea84ee01b1519cbecb02e19))
* secret store for session environment variables ([9987444](https://github.com/darioblanco/pulpo/commit/998744431ce58670fa44d5aadf3ee57fdba7cb37))
* Windows support — Docker-only sessions, release pipeline ([1959e24](https://github.com/darioblanco/pulpo/commit/1959e2417b7e33bde483037805834a66a6b6e900))
* worktree branch names, cleanup logging, and documentation ([4b59596](https://github.com/darioblanco/pulpo/commit/4b59596c0a01d656069e45e5641f04d74a91f873))
* worktree UI indicators and improved workdir validation ([bf13fae](https://github.com/darioblanco/pulpo/commit/bf13fae5860246a5f569a538ec6e2b88a9015481))


### Bug Fixes

* CI clippy errors + update ROADMAP with shipped features ([3e53490](https://github.com/darioblanco/pulpo/commit/3e53490774ac6a890990837096a35f7516be2942))
* CLI no-args spawning, resume self-collision, wrap_command quoting, and ls UX ([ea2c2b6](https://github.com/darioblanco/pulpo/commit/ea2c2b6f92b875b204ce2d036d42493d4558c60a))
* detect instantly-dying sessions before attach on spawn/resume ([8180f33](https://github.com/darioblanco/pulpo/commit/8180f33ed92d9c002eff1231f35ecabdf9398a77))
* Docker attach support and update --sandbox → --runtime docs ([9742216](https://github.com/darioblanco/pulpo/commit/974221633f556f831122e591291c2cf4617d23e5))
* error on duplicate env var when injecting multiple secrets ([842eea0](https://github.com/darioblanco/pulpo/commit/842eea05b29b1c1bc34aa5de528bcb551246d655))
* escape SQL wildcards in schedule run history LIKE query ([205b859](https://github.com/darioblanco/pulpo/commit/205b859d32138488b7fd3933c38a3b6cd6f9db71))
* get_session prefers live sessions over terminal ones ([48146ce](https://github.com/darioblanco/pulpo/commit/48146ce1970189870b034b95dbbcbc89a2f2ae5b))
* poll session liveness with retries instead of single 500ms sleep ([99ce1f0](https://github.com/darioblanco/pulpo/commit/99ce1f0f569b5837ad386805e6b9a09faa0855b9))
* reject secret values containing newlines or null bytes ([7b76663](https://github.com/darioblanco/pulpo/commit/7b76663a0c7b0667f915e1ef83320b61283659e1))
* show worktree info message when spawning with --worktree ([d60f37d](https://github.com/darioblanco/pulpo/commit/d60f37dfc61d2e1fbe395a067fd6850de4cad851))
* suppress coverage-gated unused import warnings ([eb5ebe2](https://github.com/darioblanco/pulpo/commit/eb5ebe2cc81a30f02291da6fa7ed74b769c86cfb))
* validate workdir exists before spawn/resume (tmux only) ([b8e67c7](https://github.com/darioblanco/pulpo/commit/b8e67c76815427d450e35093f7c9771682596ac8))

## [0.0.26](https://github.com/darioblanco/pulpo/compare/v0.0.25...v0.0.26) (2026-03-18)


### Features

* built-in scheduler engine (Phase 3) ([8f6ef8c](https://github.com/darioblanco/pulpo/commit/8f6ef8c2b4b9ce3c1545e20a507b97264f14b9a8))
* Docker runtime backend — run sessions in isolated containers ([7576dd2](https://github.com/darioblanco/pulpo/commit/7576dd203c6d3a85d35c0d3be90dd85d5b1f7b7f))
* git worktree support for isolated parallel sessions (Phase 4) ([48fbb40](https://github.com/darioblanco/pulpo/commit/48fbb4042c7493e8411e75f202d5110708b175c2))
* schedule CLI migration + dashboard UI (Phase 3) ([575ca4c](https://github.com/darioblanco/pulpo/commit/575ca4c5dfc5f451fe0efff470c8ed0b22f2857f))


### Bug Fixes

* fallback shell respects $SHELL, skip Claude teammate sessions ([ae682a7](https://github.com/darioblanco/pulpo/commit/ae682a72155b7faea7ed28fda259376f04db0c11))
* update brew caveats — agents are recommended, not required ([028dfaa](https://github.com/darioblanco/pulpo/commit/028dfaa69451d5d0ee36535869096a5d66c1218d))
* use login shell (bash -l -c) for tmux sessions ([7b5ba00](https://github.com/darioblanco/pulpo/commit/7b5ba0050d51128cc1968a77c137663cbd066cfe))

## [0.0.25](https://github.com/darioblanco/pulpo/compare/v0.0.24...v0.0.25) (2026-03-18)


### Features

* add PWA support — installable app with service worker ([9491d56](https://github.com/darioblanco/pulpo/commit/9491d566a58e09ca3b3efafedc6ca0d5a460e7cc))
* add session detail page with intervention history ([e5fe2c3](https://github.com/darioblanco/pulpo/commit/e5fe2c3f7d5b5e42f28409af184b6ec7fb18e12b))
* add Web Push notifications for session events ([e021946](https://github.com/darioblanco/pulpo/commit/e0219460723727d7ee0a9ae3ca17d9d7035ed3f4))
* deep UX & architecture improvements ([cf91f81](https://github.com/darioblanco/pulpo/commit/cf91f8146389c281c2e250633305a8cc157e9e02))
* default-to-shell spawn (P1.3) ([40c58db](https://github.com/darioblanco/pulpo/commit/40c58db21af42048caad26aab0e129dfd3273278))
* fleet dashboard — unified "All" tab showing sessions across nodes ([9d46fa5](https://github.com/darioblanco/pulpo/commit/9d46fa5028bc25a916b710fd66665e033261089c))
* mobile UX polish — touch targets, info visibility, fullscreen terminal ([8435933](https://github.com/darioblanco/pulpo/commit/84359337e774f2db355b7351afbcdc7d24c0a03f))
* Phase 2 — seamless remote spawn ([d408ae5](https://github.com/darioblanco/pulpo/commit/d408ae5061395f78bf0a78edcf438e72cf20ce10))
* show node hardware info in dashboard tabs and session lists ([e391eaa](https://github.com/darioblanco/pulpo/commit/e391eaaef4aaca27a3304b19797dc12786417fbf))


### Bug Fixes

* use try_get for idle_threshold_secs to avoid SQLite column cache panic ([62711eb](https://github.com/darioblanco/pulpo/commit/62711eb5360155003ad93a5feab45775df8fb582))

## [0.0.24](https://github.com/darioblanco/pulpo/compare/v0.0.23...v0.0.24) (2026-03-17)


### Features

* rename Finished→Ready status + auto-adopt external tmux sessions ([12a0f05](https://github.com/darioblanco/pulpo/commit/12a0f058789dd7a9b24fc7ec4c0af5eaca130bc3))


### Bug Fixes

* add partial unique index to prevent duplicate live session names ([60b71fb](https://github.com/darioblanco/pulpo/commit/60b71fb28da4e3241244ab2db6e4b7dc9b34d120))
* enable tmux clipboard and passthrough for image paste support ([6c12972](https://github.com/darioblanco/pulpo/commit/6c12972fb56b21282df102cb61713ce15467404e))
* remove window-size manual — let tmux auto-size to PTY client ([9421b4d](https://github.com/darioblanco/pulpo/commit/9421b4d038be4668defb7121f3f18e68d8e2551b))
* resize script PTY directly via its TTY device fd ([27bcaa6](https://github.com/darioblanco/pulpo/commit/27bcaa68944f79015dd8df9c1e891bbcab6ca86c))
* revert to script-based PTY bridge (pty-process grantpt fails on macOS daemons) ([461bd6f](https://github.com/darioblanco/pulpo/commit/461bd6f8195fc7e99462ca3c3d2f7ae2e9aa8619))
* set window-size manual on every resize, not just session creation ([712fae8](https://github.com/darioblanco/pulpo/commit/712fae89305867812c2f5e6336189ffa51ddffa1))

## [0.0.23](https://github.com/darioblanco/pulpo/compare/v0.0.22...v0.0.23) (2026-03-14)


### Features

* always use Ghostty terminal, auto-resume for lost/ready sessions ([5bbd2ae](https://github.com/darioblanco/pulpo/commit/5bbd2ae72f6eaa5897442bf1cf91734ca693e142))
* show git hash in version output to distinguish source vs release ([79e242f](https://github.com/darioblanco/pulpo/commit/79e242f9b9cfae87d84b364cb2b176cf7d5adcdd))
* **web:** make Ocean the home page and use TerminalView in attach modal ([5c11516](https://github.com/darioblanco/pulpo/commit/5c115168525c73fe00cf196f6defdaea743adbfa))


### Bug Fixes

* prevent resume from creating name collision with active session ([5efc1f4](https://github.com/darioblanco/pulpo/commit/5efc1f4fae6fee3a29b5b8ec7af3e47d609f7454))
* reject duplicate session names among active sessions ([1a8b6c8](https://github.com/darioblanco/pulpo/commit/1a8b6c874e51b30106ba4d9d73ba7807bda0c957))
* resize tmux window when browser terminal resizes ([51e0b9d](https://github.com/darioblanco/pulpo/commit/51e0b9dbcc4fd9fdbd2e15bcbc10e325b0ed3622))
* set tmux window-size manual so web terminal resize fills viewport ([fb7a61e](https://github.com/darioblanco/pulpo/commit/fb7a61ef5abacd706ded01388d4686962a02b6da))
* use pty-process into_split instead of tokio::io::split ([5f63fed](https://github.com/darioblanco/pulpo/commit/5f63fed69dc8d0e42b8e4ee458926c60724a58d8))
* **web:** terminal fills modal dialog instead of using dashboard card size ([e638fed](https://github.com/darioblanco/pulpo/commit/e638fed0af43c8311bb59b146737e35ad379320f))

## [0.0.22](https://github.com/darioblanco/pulpo/compare/v0.0.21...v0.0.22) (2026-03-14)


### Features

* **docs:** add dark theme matching Pulpo web UI color scheme ([293c2da](https://github.com/darioblanco/pulpo/commit/293c2da0f35a5a1701fc5365048bd8664d551183))


### Bug Fixes

* escape backticks in bash -c wrapper to prevent command substitution ([1452504](https://github.com/darioblanco/pulpo/commit/14525041b7978dbc8d2b5a3fbd499f37e1c27365))

## [0.0.21](https://github.com/darioblanco/pulpo/compare/v0.0.20...v0.0.21) (2026-03-13)


### Bug Fixes

* add staleness grace period to prevent race on fresh spawns ([b116865](https://github.com/darioblanco/pulpo/commit/b1168653a58933c1fa388297045219ffb0969f1d))
* detect lost sessions after reboot, require session name, auto-attach on spawn/resume ([b698c2d](https://github.com/darioblanco/pulpo/commit/b698c2d272a04bc6c97ef580357d0501536cf48f))
* gracefully degrade when tailscale serve is unavailable ([0dd466e](https://github.com/darioblanco/pulpo/commit/0dd466ebded81441dcedbdcde0e5ba07f6a1a1aa))

## [0.0.20](https://github.com/darioblanco/pulpo/compare/v0.0.19...v0.0.20) (2026-03-12)


### Features

* **watchdog:** detect idle via sustained output silence, not just patterns ([6821344](https://github.com/darioblanco/pulpo/commit/682134403f0d81b98c2981360b6eed3504b91fbc))


### Bug Fixes

* rename sprite files to match new session status names ([e19b47c](https://github.com/darioblanco/pulpo/commit/e19b47c1c4b7c89dfa2ada2e46f8a967463b22f2))

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
* **session:** ready detection on agent exit marker ([5d4c1d2](https://github.com/darioblanco/pulpo/commit/5d4c1d2de95d6a82f3dea6af30bf9dc5b05149c3))
* **session:** ready TTL cleanup and resume semantics ([36ad150](https://github.com/darioblanco/pulpo/commit/36ad15037374f57b68bda928b5aec5de9fd27fdb))
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
