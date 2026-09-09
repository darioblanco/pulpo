//! The harness-agnostic fallback adapter: no rewrite, no resume id, no events.
//!
//! Always matches (last in [`super::HarnessRegistry`]'s priority order) — every
//! command that no specific adapter claims runs exactly as it does today, with the
//! watchdog's scrollback heuristics still governing its state.

use anyhow::Result;

use super::{HarnessAdapter, HarnessEvent, SpawnContext, SpawnPlan};

pub struct GenericAdapter;

impl HarnessAdapter for GenericAdapter {
    fn id(&self) -> &'static str {
        "generic"
    }

    fn matches(&self, _argv0: &str) -> bool {
        true
    }

    fn prepare_spawn(&self, ctx: &SpawnContext) -> Result<SpawnPlan> {
        Ok(SpawnPlan::unchanged(ctx.command))
    }

    fn resume_command(&self, _original_command: &str, _harness_session_id: &str) -> Option<String> {
        None
    }

    fn parse_event(&self, _raw: &serde_json::Value) -> Result<Option<HarnessEvent>> {
        Ok(None)
    }

    fn emits_events(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_generic_matches_anything() {
        let adapter = GenericAdapter;
        assert!(adapter.matches("claude"));
        assert!(adapter.matches("bash"));
        assert!(adapter.matches(""));
    }

    #[test]
    fn test_generic_id() {
        assert_eq!(GenericAdapter.id(), "generic");
    }

    #[test]
    fn test_generic_prepare_spawn_is_noop() {
        let ctx = SpawnContext {
            session_id: "s1",
            session_name: "sess",
            workdir: "/tmp",
            command: "bash -lc 'echo hi'",
            data_dir: Path::new("/tmp/data"),
        };
        let plan = GenericAdapter.prepare_spawn(&ctx).unwrap();
        assert_eq!(plan.command, "bash -lc 'echo hi'");
        assert!(plan.env.is_empty());
        assert!(plan.files.is_empty());
        assert!(plan.harness_session_id.is_none());
    }

    #[test]
    fn test_generic_resume_command_is_none() {
        assert!(GenericAdapter.resume_command("bash", "any-id").is_none());
    }

    #[test]
    fn test_generic_parse_event_is_none() {
        let raw = serde_json::json!({"hook_event_name": "Stop"});
        assert!(GenericAdapter.parse_event(&raw).unwrap().is_none());
    }

    #[test]
    fn test_generic_does_not_emit_events() {
        assert!(!GenericAdapter.emits_events());
    }
}
