use pulpo_common::guard::{GuardConfig, GuardPreset};
use pulpo_common::session::{Provider, SessionMode};

/// Provider-agnostic parameter set for spawning an agent session.
#[derive(Debug, Clone, Default)]
pub struct SpawnParams {
    pub prompt: String,
    pub guards: GuardConfig,
    pub explicit_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub output_format: Option<String>,
    /// Provider-native worktree isolation (e.g. Claude `--worktree <name>`).
    /// Only supported by providers with built-in worktree support.
    pub worktree: Option<String>,
    /// Conversation ID for resuming a previous session.
    pub conversation_id: Option<String>,
}

/// POSIX single-quote shell escaping: wraps in single quotes,
/// escaping any interior `'` as `'\''`.
pub fn shell_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

pub fn is_unrestricted(guards: &GuardConfig) -> bool {
    guards.preset == GuardPreset::Unrestricted
}

/// Capabilities that a provider supports for `SpawnParams` fields.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ProviderCapabilities {
    pub model: bool,
    pub system_prompt: bool,
    pub allowed_tools: bool,
    pub max_turns: bool,
    pub max_budget_usd: bool,
    pub output_format: bool,
    pub worktree: bool,
    pub guard_preset: bool,
    pub resume: bool,
}

/// Return the capability set for a given provider.
#[must_use]
pub const fn provider_capabilities(provider: Provider) -> ProviderCapabilities {
    match provider {
        Provider::Claude => ProviderCapabilities {
            model: true,
            system_prompt: true,
            allowed_tools: true,
            max_turns: true,
            max_budget_usd: true,
            output_format: true,
            worktree: true,
            guard_preset: true,
            resume: true,
        },
        Provider::Codex => ProviderCapabilities {
            model: true,
            system_prompt: false,
            allowed_tools: false,
            max_turns: false,
            max_budget_usd: false,
            output_format: false,
            worktree: false,
            guard_preset: false,
            resume: true,
        },
        Provider::OpenCode => ProviderCapabilities {
            model: false,
            system_prompt: false,
            allowed_tools: false,
            max_turns: false,
            max_budget_usd: false,
            output_format: true,
            worktree: false,
            guard_preset: false,
            resume: false,
        },
    }
}

/// Check which requested params are unsupported by the provider and return warnings.
#[must_use]
pub fn check_capability_warnings(provider: Provider, params: &SpawnParams) -> Vec<String> {
    let caps = provider_capabilities(provider);
    let name = provider.to_string();
    let mut warnings = Vec::new();

    if !caps.model && params.model.is_some() {
        warnings.push(format!("{name} does not support --model; value ignored"));
    }
    if !caps.system_prompt && params.system_prompt.is_some() {
        warnings.push(format!(
            "{name} does not support --system-prompt; value ignored"
        ));
    }
    if !caps.allowed_tools && params.explicit_tools.is_some() {
        warnings.push(format!(
            "{name} does not support --allowed-tools; value ignored"
        ));
    }
    if !caps.max_turns && params.max_turns.is_some() {
        warnings.push(format!(
            "{name} does not support --max-turns; value ignored"
        ));
    }
    if !caps.max_budget_usd && params.max_budget_usd.is_some() {
        warnings.push(format!(
            "{name} does not support --max-budget-usd; value ignored"
        ));
    }
    if !caps.output_format && params.output_format.is_some() {
        warnings.push(format!(
            "{name} does not support --output-format; value ignored"
        ));
    }
    if !caps.worktree && params.worktree.is_some() {
        warnings.push(format!("{name} does not support --worktree; value ignored"));
    }

    warnings
}

/// Build flags for the given provider and session mode.
pub fn build_flags(provider: Provider, mode: SessionMode, params: &SpawnParams) -> Vec<String> {
    match (provider, mode) {
        (Provider::Claude, SessionMode::Autonomous) => build_claude_flags(params),
        (Provider::Claude, SessionMode::Interactive) => build_claude_interactive_flags(params),
        (Provider::Codex, SessionMode::Autonomous) => build_codex_flags(params),
        (Provider::Codex, SessionMode::Interactive) => build_codex_interactive_flags(params),
        (Provider::OpenCode, SessionMode::Autonomous) => build_opencode_flags(params),
        (Provider::OpenCode, SessionMode::Interactive) => build_opencode_interactive_flags(params),
    }
}

// -- Claude flag builders --

/// Build the common flags shared between Claude autonomous and interactive modes.
fn claude_common_flags(params: &SpawnParams) -> Vec<String> {
    let mut flags = Vec::new();
    if let Some(w) = &params.worktree {
        flags.push("--worktree".into());
        flags.push(w.clone());
    }
    if let Some(m) = &params.model {
        flags.push("--model".into());
        flags.push(m.clone());
    }
    if let Some(turns) = params.max_turns {
        flags.push("--max-turns".into());
        flags.push(turns.to_string());
    }
    if let Some(budget) = params.max_budget_usd {
        flags.push("--max-budget-usd".into());
        flags.push(budget.to_string());
    }
    flags
}

/// Build permission flags (--allowedTools or --dangerously-skip-permissions).
pub fn claude_permission_flags(params: &SpawnParams) -> Vec<String> {
    let mut flags = Vec::new();
    if is_unrestricted(&params.guards) && params.explicit_tools.is_none() {
        flags.push("--dangerously-skip-permissions".into());
    } else {
        let tools = params.explicit_tools.as_ref().map_or_else(
            || {
                let mut default_tools = vec![
                    "Edit".to_owned(),
                    "Write".to_owned(),
                    "Read".to_owned(),
                    "Glob".to_owned(),
                    "Grep".to_owned(),
                ];
                match params.guards.preset {
                    GuardPreset::Standard | GuardPreset::Unrestricted => {
                        default_tools.push("Bash".into());
                    }
                    GuardPreset::Strict => {}
                }
                default_tools
            },
            Clone::clone,
        );
        if !tools.is_empty() {
            flags.push("--allowedTools".into());
            flags.push(tools.join(","));
        }
    }
    flags
}

/// Build system prompt flags.
fn claude_system_prompt_flags(params: &SpawnParams) -> Vec<String> {
    let mut flags = Vec::new();
    if let Some(sp) = &params.system_prompt {
        flags.push("--append-system-prompt".into());
        flags.push(shell_escape(sp));
    }
    flags
}

/// Build flags for Claude autonomous mode (`-p` for non-interactive execution).
pub fn build_claude_flags(params: &SpawnParams) -> Vec<String> {
    let mut flags = Vec::new();
    if let Some(id) = &params.conversation_id {
        flags.push("--resume".into());
        flags.push(id.clone());
    }
    flags.push("-p".into());
    flags.push(shell_escape(&params.prompt));
    flags.extend(claude_permission_flags(params));
    flags.extend(claude_common_flags(params));
    flags.extend(claude_system_prompt_flags(params));
    if let Some(fmt) = &params.output_format {
        flags.push("--output-format".into());
        flags.push(fmt.clone());
    }
    flags
}

/// Build flags for Claude interactive mode (positional prompt, no `-p`).
pub fn build_claude_interactive_flags(params: &SpawnParams) -> Vec<String> {
    if let Some(id) = &params.conversation_id {
        // Resume: --resume <id> + model + permissions only.
        // Skip worktree, max-turns, max-budget-usd, system-prompt (inherited).
        let mut flags = vec!["--resume".into(), id.clone()];
        if let Some(m) = &params.model {
            flags.push("--model".into());
            flags.push(m.clone());
        }
        flags.extend(claude_permission_flags(params));
        return flags;
    }
    let mut flags = vec![shell_escape(&params.prompt)];
    flags.extend(claude_permission_flags(params));
    flags.extend(claude_common_flags(params));
    flags.extend(claude_system_prompt_flags(params));
    // --output-format not supported in interactive mode
    flags
}

// -- Codex flag builders --

/// Build flags for Codex autonomous mode (`-q` for non-interactive execution).
pub fn build_codex_flags(params: &SpawnParams) -> Vec<String> {
    if let Some(id) = &params.conversation_id {
        let mut flags = vec![
            "exec".into(),
            "resume".into(),
            id.clone(),
            shell_escape(&params.prompt),
        ];
        if let Some(m) = &params.model {
            flags.push("--model".into());
            flags.push(m.clone());
        }
        return flags;
    }
    let mut flags = vec!["-q".into(), shell_escape(&params.prompt)];
    if let Some(m) = &params.model {
        flags.push("--model".into());
        flags.push(m.clone());
    }
    flags
}

/// Build flags for Codex interactive mode (positional prompt, no `-q`).
pub fn build_codex_interactive_flags(params: &SpawnParams) -> Vec<String> {
    if let Some(id) = &params.conversation_id {
        let mut flags = vec!["resume".into(), id.clone()];
        if let Some(m) = &params.model {
            flags.push("--model".into());
            flags.push(m.clone());
        }
        return flags;
    }
    let mut flags = vec!["--full-auto".into(), shell_escape(&params.prompt)];
    if let Some(m) = &params.model {
        flags.push("--model".into());
        flags.push(m.clone());
    }
    flags
}

// -- OpenCode flag builders --

/// Build flags for `OpenCode` autonomous mode (`-p` for prompt).
pub fn build_opencode_flags(params: &SpawnParams) -> Vec<String> {
    let mut flags = vec!["-p".into(), shell_escape(&params.prompt)];
    if let Some(fmt) = &params.output_format {
        flags.push("-f".into());
        flags.push(fmt.clone());
    }
    flags
}

/// Build flags for `OpenCode` interactive mode (no `-p`, just launch).
/// `OpenCode` doesn't support passing a prompt in interactive mode,
/// so we pass only the output format if set.
pub fn build_opencode_interactive_flags(params: &SpawnParams) -> Vec<String> {
    let mut flags = Vec::new();
    if let Some(fmt) = &params.output_format {
        flags.push("-f".into());
        flags.push(fmt.clone());
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(prompt: &str, guards: GuardConfig) -> SpawnParams {
        SpawnParams {
            prompt: prompt.into(),
            guards,
            ..SpawnParams::default()
        }
    }

    fn strict_guards() -> GuardConfig {
        GuardConfig {
            preset: GuardPreset::Strict,
        }
    }

    fn standard_guards() -> GuardConfig {
        GuardConfig::default()
    }

    fn unrestricted_guards() -> GuardConfig {
        GuardConfig {
            preset: GuardPreset::Unrestricted,
        }
    }

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn test_shell_escape_single_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_escape_double_quotes() {
        assert_eq!(shell_escape("say \"hi\""), "'say \"hi\"'");
    }

    #[test]
    fn test_shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn test_claude_flags_unrestricted() {
        let p = params("Fix bug", unrestricted_guards());
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"-p".into()));
        assert!(flags.contains(&"'Fix bug'".into()));
    }

    #[test]
    fn test_claude_flags_strict() {
        let p = params("Fix bug", strict_guards());
        let flags = build_claude_flags(&p);
        assert!(!flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--allowedTools".into()));
        let tools_idx = flags.iter().position(|f| f == "--allowedTools").unwrap();
        let tools = &flags[tools_idx + 1];
        assert!(!tools.contains("Bash"));
        assert!(tools.contains("Read"));
        assert!(tools.contains("Edit"));
    }

    #[test]
    fn test_claude_flags_standard() {
        let p = params("test", standard_guards());
        let flags = build_claude_flags(&p);
        assert!(!flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--allowedTools".into()));
        let tools_idx = flags.iter().position(|f| f == "--allowedTools").unwrap();
        let tools = &flags[tools_idx + 1];
        assert!(tools.contains("Bash"));
        assert!(tools.contains("Read"));
    }

    #[test]
    fn test_codex_flags() {
        let p = params("test", standard_guards());
        let flags = build_codex_flags(&p);
        assert!(flags.contains(&"-q".into()));
        assert!(flags.contains(&"'test'".into()));
    }

    #[test]
    fn test_codex_interactive_flags() {
        let p = params("test", standard_guards());
        let flags = build_codex_interactive_flags(&p);
        assert!(flags.contains(&"'test'".into()));
        assert!(!flags.contains(&"-q".into()));
        assert!(flags.contains(&"--full-auto".into()));
    }

    #[test]
    fn test_codex_flags_with_model() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            model: Some("gpt-4".into()),
            ..SpawnParams::default()
        };
        let flags = build_codex_flags(&p);
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"gpt-4".into()));
        let iflags = build_codex_interactive_flags(&p);
        assert!(iflags.contains(&"--model".into()));
        assert!(iflags.contains(&"gpt-4".into()));
    }

    #[test]
    fn test_claude_flags_with_explicit_tools() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            explicit_tools: Some(vec!["Read".into(), "Grep".into()]),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        let tools_idx = flags.iter().position(|f| f == "--allowedTools").unwrap();
        let tools = &flags[tools_idx + 1];
        assert_eq!(tools, "Read,Grep");
        assert!(!tools.contains("Bash"));
    }

    #[test]
    fn test_claude_flags_unrestricted_with_explicit_tools() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: unrestricted_guards(),
            explicit_tools: Some(vec!["Read".into()]),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(!flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--allowedTools".into()));
    }

    #[test]
    fn test_claude_flags_with_model() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            model: Some("opus".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"opus".into()));
    }

    #[test]
    fn test_claude_flags_unrestricted_with_model() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: unrestricted_guards(),
            model: Some("sonnet".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"sonnet".into()));
    }

    #[test]
    fn test_claude_flags_with_system_prompt() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            system_prompt: Some("Be concise".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--append-system-prompt".into()));
        assert!(flags.contains(&"'Be concise'".into()));
    }

    #[test]
    fn test_claude_flags_unrestricted_with_system_prompt() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: unrestricted_guards(),
            system_prompt: Some("Be concise".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--append-system-prompt".into()));
    }

    #[test]
    fn test_claude_flags_all_new_flags() {
        let p = SpawnParams {
            prompt: "Fix it".into(),
            guards: standard_guards(),
            explicit_tools: Some(vec!["Read".into(), "Write".into()]),
            model: Some("opus".into()),
            system_prompt: Some("Review only".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"opus".into()));
        assert!(flags.contains(&"--allowedTools".into()));
        assert!(flags.contains(&"--append-system-prompt".into()));
        let tools_idx = flags.iter().position(|f| f == "--allowedTools").unwrap();
        assert_eq!(flags[tools_idx + 1], "Read,Write");
    }

    #[test]
    fn test_claude_flags_with_max_turns() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            max_turns: Some(10),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--max-turns".into()));
        assert!(flags.contains(&"10".into()));
    }

    #[test]
    fn test_claude_flags_with_budget() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            max_budget_usd: Some(5.0),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--max-budget-usd".into()));
        assert!(flags.contains(&"5".into()));
    }

    #[test]
    fn test_claude_flags_with_output_format() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            output_format: Some("json".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--output-format".into()));
        assert!(flags.contains(&"json".into()));
    }

    #[test]
    fn test_claude_interactive_flags_no_p() {
        let p = params("Fix bug", standard_guards());
        let flags = build_claude_interactive_flags(&p);
        assert!(!flags.contains(&"-p".into()));
        assert!(flags.contains(&"'Fix bug'".into()));
        assert!(flags.contains(&"--allowedTools".into()));
    }

    #[test]
    fn test_claude_interactive_flags_unrestricted() {
        let p = params("test", unrestricted_guards());
        let flags = build_claude_interactive_flags(&p);
        assert!(!flags.contains(&"-p".into()));
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
    }

    #[test]
    fn test_claude_worktree_flag_in_autonomous() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            worktree: Some("my-session".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        assert!(flags.contains(&"--worktree".into()));
        assert!(flags.contains(&"my-session".into()));
    }

    #[test]
    fn test_claude_worktree_flag_in_interactive() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            worktree: Some("my-session".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_interactive_flags(&p);
        assert!(flags.contains(&"--worktree".into()));
        assert!(flags.contains(&"my-session".into()));
        assert!(!flags.contains(&"-p".into()));
    }

    #[test]
    fn test_claude_no_worktree_flag_when_none() {
        let p = params("test", standard_guards());
        let flags = build_claude_flags(&p);
        assert!(!flags.contains(&"--worktree".into()));
    }

    #[test]
    fn test_codex_ignores_worktree() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            worktree: Some("my-session".into()),
            ..SpawnParams::default()
        };
        let flags = build_codex_flags(&p);
        assert!(!flags.contains(&"--worktree".into()));
        let iflags = build_codex_interactive_flags(&p);
        assert!(!iflags.contains(&"--worktree".into()));
    }

    #[test]
    fn test_codex_interactive_full_auto_is_first() {
        let p = params("test", standard_guards());
        let flags = build_codex_interactive_flags(&p);
        assert_eq!(flags[0], "--full-auto");
    }

    #[test]
    fn test_codex_autonomous_no_full_auto() {
        let p = params("test", standard_guards());
        let flags = build_codex_flags(&p);
        assert!(!flags.contains(&"--full-auto".into()));
    }

    #[test]
    fn test_claude_interactive_flags_no_output_format() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            output_format: Some("json".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_interactive_flags(&p);
        assert!(!flags.contains(&"--output-format".into()));
    }

    #[test]
    fn test_build_flags_claude_autonomous() {
        let p = params("test", unrestricted_guards());
        let flags = build_flags(Provider::Claude, SessionMode::Autonomous, &p);
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
    }

    #[test]
    fn test_build_flags_claude_interactive() {
        let p = params("test", standard_guards());
        let flags = build_flags(Provider::Claude, SessionMode::Interactive, &p);
        assert!(flags.contains(&"--allowedTools".into()));
        assert!(!flags.contains(&"-p".into()));
    }

    #[test]
    fn test_build_flags_codex_autonomous() {
        let p = params("test", standard_guards());
        let flags = build_flags(Provider::Codex, SessionMode::Autonomous, &p);
        assert!(flags.contains(&"-q".into()));
    }

    #[test]
    fn test_build_flags_codex_interactive() {
        let p = params("test", standard_guards());
        let flags = build_flags(Provider::Codex, SessionMode::Interactive, &p);
        assert!(flags.contains(&"--full-auto".into()));
    }

    #[test]
    fn test_is_unrestricted_true() {
        let guards = unrestricted_guards();
        assert!(is_unrestricted(&guards));
    }

    #[test]
    fn test_is_unrestricted_false_standard() {
        let guards = standard_guards();
        assert!(!is_unrestricted(&guards));
    }

    #[test]
    fn test_is_unrestricted_false_strict() {
        let guards = strict_guards();
        assert!(!is_unrestricted(&guards));
    }

    #[test]
    fn test_spawn_params_default() {
        let p = SpawnParams::default();
        assert!(p.prompt.is_empty());
        assert!(p.explicit_tools.is_none());
        assert!(p.model.is_none());
        assert!(p.system_prompt.is_none());
        assert!(p.max_turns.is_none());
        assert!(p.max_budget_usd.is_none());
        assert!(p.output_format.is_none());
        assert!(p.conversation_id.is_none());
    }

    #[test]
    fn test_spawn_params_debug() {
        let p = SpawnParams {
            prompt: "test".into(),
            conversation_id: Some("conv-123".into()),
            ..SpawnParams::default()
        };
        let debug = format!("{p:?}");
        assert!(debug.contains("test"));
        assert!(debug.contains("conv-123"));
    }

    #[test]
    fn test_spawn_params_clone() {
        let p = SpawnParams {
            prompt: "test".into(),
            max_turns: Some(5),
            conversation_id: Some("conv-abc".into()),
            ..SpawnParams::default()
        };
        #[allow(clippy::redundant_clone)]
        let p2 = p.clone();
        assert_eq!(p2.prompt, "test");
        assert_eq!(p2.max_turns, Some(5));
        assert_eq!(p2.conversation_id, Some("conv-abc".into()));
    }

    #[test]
    fn test_claude_interactive_resume_flags() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            model: Some("sonnet".into()),
            conversation_id: Some("conv-123".into()),
            worktree: Some("my-session".into()),
            max_turns: Some(10),
            system_prompt: Some("Be concise".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_interactive_flags(&p);
        assert_eq!(flags[0], "--resume");
        assert_eq!(flags[1], "conv-123");
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"sonnet".into()));
        assert!(flags.contains(&"--allowedTools".into()));
        // Resume skips worktree, max-turns, system-prompt
        assert!(!flags.contains(&"--worktree".into()));
        assert!(!flags.contains(&"--max-turns".into()));
        assert!(!flags.contains(&"--append-system-prompt".into()));
    }

    #[test]
    fn test_claude_interactive_resume_unrestricted() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: unrestricted_guards(),
            conversation_id: Some("conv-456".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_interactive_flags(&p);
        assert_eq!(flags[0], "--resume");
        assert_eq!(flags[1], "conv-456");
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(!flags.contains(&"--allowedTools".into()));
    }

    #[test]
    fn test_claude_autonomous_resume_flags() {
        let p = SpawnParams {
            prompt: "Fix bug".into(),
            guards: standard_guards(),
            model: Some("opus".into()),
            conversation_id: Some("conv-789".into()),
            system_prompt: Some("Review only".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_flags(&p);
        // --resume prepended before -p
        assert_eq!(flags[0], "--resume");
        assert_eq!(flags[1], "conv-789");
        assert!(flags.contains(&"-p".into()));
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"opus".into()));
        assert!(flags.contains(&"--allowedTools".into()));
        assert!(flags.contains(&"--append-system-prompt".into()));
    }

    #[test]
    fn test_claude_interactive_resume_no_model() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            conversation_id: Some("conv-000".into()),
            ..SpawnParams::default()
        };
        let flags = build_claude_interactive_flags(&p);
        assert_eq!(flags[0], "--resume");
        assert_eq!(flags[1], "conv-000");
        assert!(!flags.contains(&"--model".into()));
        assert!(flags.contains(&"--allowedTools".into()));
    }

    #[test]
    fn test_codex_interactive_resume_flags() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            model: Some("gpt-4".into()),
            conversation_id: Some("conv-codex-1".into()),
            ..SpawnParams::default()
        };
        let flags = build_codex_interactive_flags(&p);
        assert_eq!(flags[0], "resume");
        assert_eq!(flags[1], "conv-codex-1");
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"gpt-4".into()));
        // Should NOT contain --full-auto or prompt
        assert!(!flags.contains(&"--full-auto".into()));
        assert!(!flags.contains(&"'test'".into()));
    }

    #[test]
    fn test_codex_autonomous_resume_flags() {
        let p = SpawnParams {
            prompt: "Fix bug".into(),
            guards: standard_guards(),
            model: Some("gpt-4".into()),
            conversation_id: Some("conv-codex-2".into()),
            ..SpawnParams::default()
        };
        let flags = build_codex_flags(&p);
        assert_eq!(flags[0], "exec");
        assert_eq!(flags[1], "resume");
        assert_eq!(flags[2], "conv-codex-2");
        assert_eq!(flags[3], "'Fix bug'");
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"gpt-4".into()));
        // Should NOT contain -q
        assert!(!flags.contains(&"-q".into()));
    }

    #[test]
    fn test_codex_interactive_resume_no_model() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            conversation_id: Some("conv-codex-3".into()),
            ..SpawnParams::default()
        };
        let flags = build_codex_interactive_flags(&p);
        assert_eq!(flags[0], "resume");
        assert_eq!(flags[1], "conv-codex-3");
        assert_eq!(flags.len(), 2);
    }

    // -- OpenCode tests --

    #[test]
    fn test_opencode_autonomous_flags() {
        let p = params("Fix the bug", standard_guards());
        let flags = build_opencode_flags(&p);
        assert_eq!(flags[0], "-p");
        assert_eq!(flags[1], "'Fix the bug'");
        assert_eq!(flags.len(), 2);
    }

    #[test]
    fn test_opencode_autonomous_with_output_format() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            output_format: Some("json".into()),
            ..SpawnParams::default()
        };
        let flags = build_opencode_flags(&p);
        assert!(flags.contains(&"-f".into()));
        assert!(flags.contains(&"json".into()));
    }

    #[test]
    fn test_opencode_interactive_flags_empty() {
        let p = params("test", standard_guards());
        let flags = build_opencode_interactive_flags(&p);
        assert!(flags.is_empty());
    }

    #[test]
    fn test_opencode_interactive_with_output_format() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            output_format: Some("text".into()),
            ..SpawnParams::default()
        };
        let flags = build_opencode_interactive_flags(&p);
        assert_eq!(flags, vec!["-f", "text"]);
    }

    #[test]
    fn test_opencode_ignores_model() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            model: Some("gpt-4".into()),
            ..SpawnParams::default()
        };
        let flags = build_opencode_flags(&p);
        assert!(!flags.contains(&"--model".into()));
        let iflags = build_opencode_interactive_flags(&p);
        assert!(!iflags.contains(&"--model".into()));
    }

    #[test]
    fn test_opencode_ignores_system_prompt() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            system_prompt: Some("Be concise".into()),
            ..SpawnParams::default()
        };
        let flags = build_opencode_flags(&p);
        assert!(!flags.contains(&"--append-system-prompt".into()));
    }

    #[test]
    fn test_opencode_ignores_worktree() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            worktree: Some("my-session".into()),
            ..SpawnParams::default()
        };
        let flags = build_opencode_flags(&p);
        assert!(!flags.contains(&"--worktree".into()));
    }

    #[test]
    fn test_build_flags_opencode_autonomous() {
        let p = params("test", standard_guards());
        let flags = build_flags(Provider::OpenCode, SessionMode::Autonomous, &p);
        assert!(flags.contains(&"-p".into()));
    }

    #[test]
    fn test_build_flags_opencode_interactive() {
        let p = params("test", standard_guards());
        let flags = build_flags(Provider::OpenCode, SessionMode::Interactive, &p);
        assert!(flags.is_empty());
    }

    // -- Provider capabilities tests --

    #[test]
    fn test_provider_capabilities_claude() {
        let caps = provider_capabilities(Provider::Claude);
        assert!(caps.model);
        assert!(caps.system_prompt);
        assert!(caps.allowed_tools);
        assert!(caps.max_turns);
        assert!(caps.max_budget_usd);
        assert!(caps.output_format);
        assert!(caps.worktree);
        assert!(caps.guard_preset);
        assert!(caps.resume);
    }

    #[test]
    fn test_provider_capabilities_codex() {
        let caps = provider_capabilities(Provider::Codex);
        assert!(caps.model);
        assert!(!caps.system_prompt);
        assert!(!caps.allowed_tools);
        assert!(!caps.max_turns);
        assert!(!caps.max_budget_usd);
        assert!(!caps.output_format);
        assert!(!caps.worktree);
        assert!(!caps.guard_preset);
        assert!(caps.resume);
    }

    #[test]
    fn test_provider_capabilities_opencode() {
        let caps = provider_capabilities(Provider::OpenCode);
        assert!(!caps.model);
        assert!(!caps.system_prompt);
        assert!(!caps.allowed_tools);
        assert!(!caps.max_turns);
        assert!(!caps.max_budget_usd);
        assert!(caps.output_format);
        assert!(!caps.worktree);
        assert!(!caps.guard_preset);
        assert!(!caps.resume);
    }

    #[test]
    fn test_provider_capabilities_debug_clone() {
        let caps = provider_capabilities(Provider::Claude);
        #[allow(clippy::redundant_clone)]
        let caps2 = caps.clone();
        let debug = format!("{caps2:?}");
        assert!(debug.contains("model: true"));
    }

    // -- Capability warnings tests --

    #[test]
    fn test_check_capability_warnings_claude_no_warnings() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            model: Some("opus".into()),
            system_prompt: Some("Be concise".into()),
            max_turns: Some(10),
            max_budget_usd: Some(5.0),
            output_format: Some("json".into()),
            worktree: Some("ws".into()),
            ..SpawnParams::default()
        };
        let warnings = check_capability_warnings(Provider::Claude, &p);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_check_capability_warnings_codex_model_ok() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            model: Some("gpt-4".into()),
            ..SpawnParams::default()
        };
        let warnings = check_capability_warnings(Provider::Codex, &p);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_check_capability_warnings_codex_unsupported() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            system_prompt: Some("Be concise".into()),
            max_turns: Some(10),
            max_budget_usd: Some(5.0),
            output_format: Some("json".into()),
            worktree: Some("ws".into()),
            explicit_tools: Some(vec!["Read".into()]),
            ..SpawnParams::default()
        };
        let warnings = check_capability_warnings(Provider::Codex, &p);
        assert_eq!(warnings.len(), 6);
        assert!(warnings.iter().any(|w| w.contains("--system-prompt")));
        assert!(warnings.iter().any(|w| w.contains("--max-turns")));
        assert!(warnings.iter().any(|w| w.contains("--max-budget-usd")));
        assert!(warnings.iter().any(|w| w.contains("--output-format")));
        assert!(warnings.iter().any(|w| w.contains("--allowed-tools")));
        assert!(warnings.iter().any(|w| w.contains("--worktree")));
    }

    #[test]
    fn test_check_capability_warnings_opencode_all() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            model: Some("gpt-4".into()),
            system_prompt: Some("Be concise".into()),
            max_turns: Some(10),
            max_budget_usd: Some(5.0),
            worktree: Some("ws".into()),
            explicit_tools: Some(vec!["Read".into()]),
            ..SpawnParams::default()
        };
        let warnings = check_capability_warnings(Provider::OpenCode, &p);
        assert_eq!(warnings.len(), 6);
        assert!(warnings.iter().any(|w| w.contains("--model")));
        assert!(warnings.iter().any(|w| w.contains("--system-prompt")));
        assert!(warnings.iter().any(|w| w.contains("--max-turns")));
        assert!(warnings.iter().any(|w| w.contains("--max-budget-usd")));
        assert!(warnings.iter().any(|w| w.contains("--worktree")));
        assert!(warnings.iter().any(|w| w.contains("--allowed-tools")));
    }

    #[test]
    fn test_check_capability_warnings_opencode_output_format_ok() {
        let p = SpawnParams {
            prompt: "test".into(),
            guards: standard_guards(),
            output_format: Some("json".into()),
            ..SpawnParams::default()
        };
        let warnings = check_capability_warnings(Provider::OpenCode, &p);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_check_capability_warnings_no_params_no_warnings() {
        let p = params("test", standard_guards());
        let warnings = check_capability_warnings(Provider::OpenCode, &p);
        assert!(warnings.is_empty());
    }
}
