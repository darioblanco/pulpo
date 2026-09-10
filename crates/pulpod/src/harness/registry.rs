//! [`HarnessRegistry`]: resolves a command line (or an explicit harness id) to a
//! [`HarnessAdapter`], `generic` last.

use std::path::Path;
use std::sync::Arc;

use super::HarnessAdapter;
use super::HarnessSignals;
use super::claude::ClaudeAdapter;
use super::codex::CodexAdapter;
use super::generic::GenericAdapter;
use super::pi::PiAdapter;

/// Holds adapters in priority order.
///
/// Resolves a command line to one via `shell_words::split` + basename of `argv[0]`
/// (handling `env FOO=bar claude`, absolute paths, etc). [`GenericAdapter`] always
/// matches last: no rewrite, no resume id, no events.
#[derive(Clone)]
pub struct HarnessRegistry {
    adapters: Vec<Arc<dyn HarnessAdapter>>,
}

impl Default for HarnessRegistry {
    fn default() -> Self {
        Self {
            adapters: vec![
                Arc::new(ClaudeAdapter),
                Arc::new(CodexAdapter),
                Arc::new(PiAdapter),
                Arc::new(GenericAdapter),
            ],
        }
    }
}

impl HarnessRegistry {
    /// Resolve a command line to the first adapter whose [`HarnessAdapter::matches`]
    /// accepts the basename of `argv[0]`. Falls back to the generic adapter when the
    /// command can't be parsed as shell words, or no adapter claims it.
    #[must_use]
    pub fn resolve(&self, command: &str) -> Arc<dyn HarnessAdapter> {
        if let Some(basename) = argv0_basename(command) {
            for adapter in &self.adapters {
                if adapter.matches(&basename) {
                    return adapter.clone();
                }
            }
        }
        self.generic()
    }

    /// Look up an adapter by its stable [`HarnessAdapter::id`] (used by the
    /// harness-events endpoint, which receives the id explicitly in the request body
    /// rather than a command line).
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<dyn HarnessAdapter>> {
        self.adapters.iter().find(|a| a.id() == id).cloned()
    }

    fn generic(&self) -> Arc<dyn HarnessAdapter> {
        self.get("generic")
            .unwrap_or_else(|| Arc::new(GenericAdapter))
    }

    /// Resolve which scrollback-heuristic signals remain safe for the watchdog to
    /// apply to a session, given its own harness id (as stored on the session) and
    /// whether its hook events are actually flowing yet (`harness_last_event_at` is
    /// set — see `watchdog::harness_owns_state`). Events not yet flowing own nothing
    /// (every heuristic stays active) — the same fallback the pre-adapter watchdog
    /// behavior had. An unrecognized or absent harness id with events flowing is
    /// treated as owning everything (the row said events arrived, so trust them).
    ///
    /// An adapter that doesn't actually emit events (`emits_events() == false`, i.e.
    /// `GenericAdapter`) can never own anything either, regardless of what
    /// `owned_signals()` would otherwise report (its trait default is "all") —
    /// `harness_last_event_at` can end up set for such a session anyway (e.g. a
    /// stray/malformed `harness-events` POST naming a non-emitting harness id), and
    /// that must never be read as "this session's heuristics are covered," since
    /// nothing is really wired up to cover them.
    #[must_use]
    pub fn owned_signals_for(
        &self,
        harness_id: Option<&str>,
        events_flowing: bool,
    ) -> HarnessSignals {
        if !events_flowing {
            return HarnessSignals::none();
        }
        harness_id
            .and_then(|id| self.get(id))
            .map_or(HarnessSignals::all(), |adapter| {
                if adapter.emits_events() {
                    adapter.owned_signals()
                } else {
                    HarnessSignals::none()
                }
            })
    }
}

/// Extract the basename of the command's effective `argv[0]`, skipping a leading
/// `env` invocation and its `VAR=value` assignments / flags (e.g. `env FOO=bar
/// claude` → `claude`). Returns `None` when the command doesn't parse as shell words
/// (e.g. unbalanced quotes) or is empty.
fn argv0_basename(command: &str) -> Option<String> {
    let words = shell_words::split(command).ok()?;
    let mut iter = words.into_iter();
    let mut first = iter.next()?;

    if Path::new(&first).file_name().and_then(|f| f.to_str()) == Some("env") {
        for word in iter {
            if word.starts_with('-') || word.contains('=') {
                continue;
            }
            first = word;
            break;
        }
    }

    Path::new(&first)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argv0_basename_plain() {
        assert_eq!(argv0_basename("claude").as_deref(), Some("claude"));
    }

    #[test]
    fn test_argv0_basename_absolute_path() {
        assert_eq!(
            argv0_basename("/usr/local/bin/claude --resume x").as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn test_argv0_basename_env_prefix() {
        assert_eq!(
            argv0_basename("env X=1 claude -p hi").as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn test_argv0_basename_env_with_multiple_assignments_and_flags() {
        assert_eq!(
            argv0_basename("env -i FOO=bar BAZ=qux codex exec 'fix'").as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn test_argv0_basename_bash() {
        assert_eq!(argv0_basename("bash").as_deref(), Some("bash"));
    }

    #[test]
    fn test_argv0_basename_empty_command() {
        assert!(argv0_basename("").is_none());
    }

    #[test]
    fn test_argv0_basename_unparseable_command() {
        // Unbalanced quote — shell_words::split fails.
        assert!(argv0_basename("claude \"unterminated").is_none());
    }

    #[test]
    fn test_registry_resolves_claude() {
        let registry = HarnessRegistry::default();
        assert_eq!(registry.resolve("claude -p hi").id(), "claude");
    }

    #[test]
    fn test_registry_resolves_claude_absolute_path() {
        let registry = HarnessRegistry::default();
        assert_eq!(
            registry.resolve("/usr/local/bin/claude --resume x").id(),
            "claude"
        );
    }

    #[test]
    fn test_registry_resolves_claude_via_env_prefix() {
        let registry = HarnessRegistry::default();
        assert_eq!(registry.resolve("env X=1 claude").id(), "claude");
    }

    #[test]
    fn test_registry_falls_back_to_generic_for_unknown_agent() {
        let registry = HarnessRegistry::default();
        assert_eq!(registry.resolve("gemini chat").id(), "generic");
    }

    #[test]
    fn test_registry_resolves_codex() {
        let registry = HarnessRegistry::default();
        assert_eq!(registry.resolve("codex exec 'fix'").id(), "codex");
    }

    #[test]
    fn test_registry_resolves_pi() {
        let registry = HarnessRegistry::default();
        assert_eq!(registry.resolve("pi -p hi").id(), "pi");
    }

    #[test]
    fn test_registry_resolves_pi_via_env_prefix() {
        let registry = HarnessRegistry::default();
        assert_eq!(registry.resolve("env X=1 pi").id(), "pi");
    }

    #[test]
    fn test_registry_does_not_resolve_npx_pi_to_pi_adapter() {
        // `npx pi ...` has argv0 basename "npx", not "pi" — pulpo's basename
        // resolution has no npx-aware unwrapping, so this intentionally falls
        // through to `GenericAdapter`. Documented gap, not a bug (see
        // `pi::tests::test_matches_does_not_match_npx_pi`).
        let registry = HarnessRegistry::default();
        assert_eq!(registry.resolve("npx pi -p hi").id(), "generic");
    }

    #[test]
    fn test_registry_falls_back_to_generic_for_bash() {
        let registry = HarnessRegistry::default();
        assert_eq!(registry.resolve("bash").id(), "generic");
    }

    #[test]
    fn test_registry_falls_back_to_generic_for_unparseable_command() {
        let registry = HarnessRegistry::default();
        assert_eq!(registry.resolve("claude \"unterminated").id(), "generic");
    }

    #[test]
    fn test_registry_get_by_id() {
        let registry = HarnessRegistry::default();
        assert_eq!(registry.get("claude").unwrap().id(), "claude");
        assert_eq!(registry.get("codex").unwrap().id(), "codex");
        assert_eq!(registry.get("pi").unwrap().id(), "pi");
        assert_eq!(registry.get("generic").unwrap().id(), "generic");
        assert!(registry.get("gemini").is_none());
    }

    // -- owned_signals_for --

    #[test]
    fn test_owned_signals_for_events_not_flowing_is_none() {
        let registry = HarnessRegistry::default();
        assert_eq!(
            registry.owned_signals_for(Some("claude"), false),
            HarnessSignals::none()
        );
    }

    #[test]
    fn test_owned_signals_for_claude_events_flowing_is_all() {
        let registry = HarnessRegistry::default();
        assert_eq!(
            registry.owned_signals_for(Some("claude"), true),
            HarnessSignals::all()
        );
    }

    #[test]
    fn test_owned_signals_for_codex_events_flowing_is_lifecycle_only() {
        let registry = HarnessRegistry::default();
        assert_eq!(
            registry.owned_signals_for(Some("codex"), true),
            HarnessSignals::lifecycle_only()
        );
    }

    #[test]
    fn test_owned_signals_for_unknown_harness_events_flowing_is_all() {
        let registry = HarnessRegistry::default();
        assert_eq!(
            registry.owned_signals_for(Some("gemini"), true),
            HarnessSignals::all()
        );
    }

    #[test]
    fn test_owned_signals_for_no_harness_id_events_flowing_is_all() {
        let registry = HarnessRegistry::default();
        assert_eq!(
            registry.owned_signals_for(None, true),
            HarnessSignals::all()
        );
    }

    #[test]
    fn test_owned_signals_for_generic_events_flowing_is_none() {
        // GenericAdapter never emits real events (`emits_events() == false`), but
        // `harness_last_event_at` can still get touched (e.g. a stray/malformed
        // `harness-events` POST naming "generic") — a non-emitting adapter must never
        // be treated as owning any watchdog heuristic just because the trait default
        // for `owned_signals()` happens to be "all". See
        // `harness::generic::tests::test_generic_does_not_emit_events`.
        let registry = HarnessRegistry::default();
        assert_eq!(
            registry.owned_signals_for(Some("generic"), true),
            HarnessSignals::none()
        );
    }
}
