use std::collections::HashMap;

use pulpo_common::guard::{EnvFilter, FileScope, GuardConfig, ShellAccess};
use pulpo_common::session::Provider;

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

pub trait GuardTranslator: Send + Sync {
    /// Build flags for autonomous mode (`-p` / `-q` for non-interactive execution).
    fn build_flags(&self, params: &SpawnParams) -> Vec<String>;
    /// Build flags for interactive mode (positional prompt, no `-p`).
    fn build_interactive_flags(&self, params: &SpawnParams) -> Vec<String>;
    fn sanitize_env(
        &self,
        guards: &GuardConfig,
        env: HashMap<String, String>,
    ) -> HashMap<String, String>;
}

pub struct ClaudeTranslator;

impl ClaudeTranslator {
    /// Build the common flags shared between autonomous and interactive modes.
    fn common_flags(params: &SpawnParams) -> Vec<String> {
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
    pub fn permission_flags(params: &SpawnParams) -> Vec<String> {
        let mut flags = Vec::new();
        if is_yolo(&params.guards) && params.explicit_tools.is_none() {
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
                    match params.guards.shell {
                        ShellAccess::Restricted | ShellAccess::Unrestricted => {
                            default_tools.push("Bash".into());
                        }
                        ShellAccess::None => {}
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
    fn system_prompt_flags(params: &SpawnParams) -> Vec<String> {
        let mut flags = Vec::new();
        if let Some(sp) = &params.system_prompt {
            flags.push("--append-system-prompt".into());
            flags.push(shell_escape(sp));
        }
        flags
    }
}

impl GuardTranslator for ClaudeTranslator {
    fn build_flags(&self, params: &SpawnParams) -> Vec<String> {
        let mut flags = vec!["-p".into(), shell_escape(&params.prompt)];
        flags.extend(Self::permission_flags(params));
        flags.extend(Self::common_flags(params));
        flags.extend(Self::system_prompt_flags(params));
        if let Some(fmt) = &params.output_format {
            flags.push("--output-format".into());
            flags.push(fmt.clone());
        }
        flags
    }

    fn build_interactive_flags(&self, params: &SpawnParams) -> Vec<String> {
        let mut flags = vec![shell_escape(&params.prompt)];
        flags.extend(Self::permission_flags(params));
        flags.extend(Self::common_flags(params));
        flags.extend(Self::system_prompt_flags(params));
        // --output-format not supported in interactive mode
        flags
    }

    fn sanitize_env(
        &self,
        guards: &GuardConfig,
        env: HashMap<String, String>,
    ) -> HashMap<String, String> {
        filter_env(&guards.env, env)
    }
}

pub struct CodexTranslator;

impl GuardTranslator for CodexTranslator {
    fn build_flags(&self, params: &SpawnParams) -> Vec<String> {
        let mut flags = vec!["-q".into(), shell_escape(&params.prompt)];
        if let Some(m) = &params.model {
            flags.push("--model".into());
            flags.push(m.clone());
        }
        flags
    }

    fn build_interactive_flags(&self, params: &SpawnParams) -> Vec<String> {
        let mut flags = vec!["--full-auto".into(), shell_escape(&params.prompt)];
        if let Some(m) = &params.model {
            flags.push("--model".into());
            flags.push(m.clone());
        }
        flags
    }

    fn sanitize_env(
        &self,
        guards: &GuardConfig,
        env: HashMap<String, String>,
    ) -> HashMap<String, String> {
        filter_env(&guards.env, env)
    }
}

pub fn translator_for(provider: Provider) -> Box<dyn GuardTranslator> {
    match provider {
        Provider::Claude => Box::new(ClaudeTranslator),
        Provider::Codex => Box::new(CodexTranslator),
    }
}

pub fn is_yolo(guards: &GuardConfig) -> bool {
    guards.file_write == FileScope::Unrestricted
        && guards.file_read == FileScope::Unrestricted
        && guards.shell == ShellAccess::Unrestricted
        && guards.network
        && guards.install_packages
        && guards.git_push
}

#[allow(clippy::implicit_hasher)]
pub fn filter_env(env_filter: &EnvFilter, env: HashMap<String, String>) -> HashMap<String, String> {
    if env_filter.allow.is_empty() && env_filter.deny.is_empty() {
        return env;
    }

    env.into_iter()
        .filter(|(key, _)| {
            // If allow list is non-empty, key must match at least one allow pattern
            let allowed =
                env_filter.allow.is_empty() || env_filter.allow.iter().any(|p| glob_match(p, key));
            // Key must not match any deny pattern
            let denied = env_filter.deny.iter().any(|p| glob_match(p, key));
            allowed && !denied
        })
        .collect()
}

/// Simple prefix-wildcard glob match: `AWS_*` matches `AWS_ACCESS_KEY_ID`.
/// If the pattern ends with `*`, it's a prefix match. Otherwise exact match.
pub fn glob_match(pattern: &str, value: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or_else(|| pattern == value, |prefix| value.starts_with(prefix))
}

#[allow(clippy::implicit_hasher)]
pub fn wrap_with_env(env: &HashMap<String, String>, command: &str) -> String {
    let mut parts = vec!["env".to_owned(), "-i".to_owned()];
    let mut keys: Vec<&String> = env.keys().collect();
    keys.sort();
    for key in keys {
        let value = &env[key];
        parts.push(format!("{key}={value}"));
    }
    parts.push(command.to_owned());
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::guard::GuardPreset;

    fn params(prompt: &str, guards: GuardConfig) -> SpawnParams {
        SpawnParams {
            prompt: prompt.into(),
            guards,
            ..SpawnParams::default()
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
    fn test_claude_translator_yolo() {
        let t = ClaudeTranslator;
        let p = params("Fix bug", GuardConfig::from_preset(GuardPreset::Yolo));
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"-p".into()));
        assert!(flags.contains(&"'Fix bug'".into()));
    }

    #[test]
    fn test_claude_translator_strict() {
        let t = ClaudeTranslator;
        let p = params("Fix bug", GuardConfig::from_preset(GuardPreset::Strict));
        let flags = t.build_flags(&p);
        assert!(!flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--allowedTools".into()));
        let tools_idx = flags.iter().position(|f| f == "--allowedTools").unwrap();
        let tools = &flags[tools_idx + 1];
        assert!(!tools.contains("Bash"));
        assert!(tools.contains("Read"));
        assert!(tools.contains("Edit"));
    }

    #[test]
    fn test_claude_translator_standard() {
        let t = ClaudeTranslator;
        let p = params("test", GuardConfig::from_preset(GuardPreset::Standard));
        let flags = t.build_flags(&p);
        assert!(!flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--allowedTools".into()));
        let tools_idx = flags.iter().position(|f| f == "--allowedTools").unwrap();
        let tools = &flags[tools_idx + 1];
        assert!(tools.contains("Bash"));
        assert!(tools.contains("Read"));
    }

    #[test]
    fn test_codex_translator_build_flags() {
        let t = CodexTranslator;
        let p = params("test", GuardConfig::from_preset(GuardPreset::Standard));
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"-q".into()));
        assert!(flags.contains(&"'test'".into()));
    }

    #[test]
    fn test_codex_translator_interactive_flags() {
        let t = CodexTranslator;
        let p = params("test", GuardConfig::from_preset(GuardPreset::Standard));
        let flags = t.build_interactive_flags(&p);
        assert!(flags.contains(&"'test'".into()));
        assert!(!flags.contains(&"-q".into()));
        assert!(flags.contains(&"--full-auto".into()));
    }

    #[test]
    fn test_codex_translator_with_model() {
        let t = CodexTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            model: Some("gpt-4".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"gpt-4".into()));
        let iflags = t.build_interactive_flags(&p);
        assert!(iflags.contains(&"--model".into()));
        assert!(iflags.contains(&"gpt-4".into()));
    }

    #[test]
    fn test_claude_translator_with_explicit_tools() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            explicit_tools: Some(vec!["Read".into(), "Grep".into()]),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        let tools_idx = flags.iter().position(|f| f == "--allowedTools").unwrap();
        let tools = &flags[tools_idx + 1];
        assert_eq!(tools, "Read,Grep");
        assert!(!tools.contains("Bash"));
    }

    #[test]
    fn test_claude_translator_yolo_with_explicit_tools() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Yolo),
            explicit_tools: Some(vec!["Read".into()]),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(!flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--allowedTools".into()));
    }

    #[test]
    fn test_claude_translator_with_model() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            model: Some("opus".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"opus".into()));
    }

    #[test]
    fn test_claude_translator_yolo_with_model() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Yolo),
            model: Some("sonnet".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"sonnet".into()));
    }

    #[test]
    fn test_claude_translator_with_system_prompt() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            system_prompt: Some("Be concise".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--append-system-prompt".into()));
        assert!(flags.contains(&"'Be concise'".into()));
    }

    #[test]
    fn test_claude_translator_yolo_with_system_prompt() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Yolo),
            system_prompt: Some("Be concise".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
        assert!(flags.contains(&"--append-system-prompt".into()));
    }

    #[test]
    fn test_claude_translator_all_new_flags() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "Fix it".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            explicit_tools: Some(vec!["Read".into(), "Write".into()]),
            model: Some("opus".into()),
            system_prompt: Some("Review only".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--model".into()));
        assert!(flags.contains(&"opus".into()));
        assert!(flags.contains(&"--allowedTools".into()));
        assert!(flags.contains(&"--append-system-prompt".into()));
        let tools_idx = flags.iter().position(|f| f == "--allowedTools").unwrap();
        assert_eq!(flags[tools_idx + 1], "Read,Write");
    }

    #[test]
    fn test_claude_translator_with_max_turns() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            max_turns: Some(10),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--max-turns".into()));
        assert!(flags.contains(&"10".into()));
    }

    #[test]
    fn test_claude_translator_with_budget() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            max_budget_usd: Some(5.0),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--max-budget-usd".into()));
        assert!(flags.contains(&"5".into()));
    }

    #[test]
    fn test_claude_translator_with_output_format() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            output_format: Some("json".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--output-format".into()));
        assert!(flags.contains(&"json".into()));
    }

    #[test]
    fn test_claude_interactive_flags_no_p() {
        let t = ClaudeTranslator;
        let p = params("Fix bug", GuardConfig::from_preset(GuardPreset::Standard));
        let flags = t.build_interactive_flags(&p);
        assert!(!flags.contains(&"-p".into()));
        assert!(flags.contains(&"'Fix bug'".into()));
        assert!(flags.contains(&"--allowedTools".into()));
    }

    #[test]
    fn test_claude_interactive_flags_yolo() {
        let t = ClaudeTranslator;
        let p = params("test", GuardConfig::from_preset(GuardPreset::Yolo));
        let flags = t.build_interactive_flags(&p);
        assert!(!flags.contains(&"-p".into()));
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
    }

    #[test]
    fn test_claude_worktree_flag_in_autonomous() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            worktree: Some("my-session".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--worktree".into()));
        assert!(flags.contains(&"my-session".into()));
    }

    #[test]
    fn test_claude_worktree_flag_in_interactive() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            worktree: Some("my-session".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_interactive_flags(&p);
        assert!(flags.contains(&"--worktree".into()));
        assert!(flags.contains(&"my-session".into()));
        assert!(!flags.contains(&"-p".into()));
    }

    #[test]
    fn test_claude_no_worktree_flag_when_none() {
        let t = ClaudeTranslator;
        let p = params("test", GuardConfig::from_preset(GuardPreset::Standard));
        let flags = t.build_flags(&p);
        assert!(!flags.contains(&"--worktree".into()));
    }

    #[test]
    fn test_codex_ignores_worktree() {
        let t = CodexTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            worktree: Some("my-session".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_flags(&p);
        assert!(!flags.contains(&"--worktree".into()));
        let iflags = t.build_interactive_flags(&p);
        assert!(!iflags.contains(&"--worktree".into()));
    }

    #[test]
    fn test_codex_interactive_full_auto_is_first() {
        let t = CodexTranslator;
        let p = params("test", GuardConfig::from_preset(GuardPreset::Standard));
        let flags = t.build_interactive_flags(&p);
        assert_eq!(flags[0], "--full-auto");
    }

    #[test]
    fn test_codex_autonomous_no_full_auto() {
        let t = CodexTranslator;
        let p = params("test", GuardConfig::from_preset(GuardPreset::Standard));
        let flags = t.build_flags(&p);
        assert!(!flags.contains(&"--full-auto".into()));
    }

    #[test]
    fn test_claude_interactive_flags_no_output_format() {
        let t = ClaudeTranslator;
        let p = SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::from_preset(GuardPreset::Standard),
            output_format: Some("json".into()),
            ..SpawnParams::default()
        };
        let flags = t.build_interactive_flags(&p);
        assert!(!flags.contains(&"--output-format".into()));
    }

    #[test]
    fn test_codex_translator_sanitize_env() {
        let t = CodexTranslator;
        let mut guards = GuardConfig::from_preset(GuardPreset::Standard);
        guards.env = EnvFilter {
            allow: vec!["PATH".into()],
            deny: vec![],
        };
        let mut env = HashMap::new();
        env.insert("PATH".into(), "/usr/bin".into());
        env.insert("SECRET".into(), "xyz".into());
        let filtered = t.sanitize_env(&guards, env);
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains_key("PATH"));
    }

    #[test]
    fn test_translator_for_claude() {
        let t = translator_for(Provider::Claude);
        let p = params("test", GuardConfig::from_preset(GuardPreset::Yolo));
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"--dangerously-skip-permissions".into()));
    }

    #[test]
    fn test_translator_for_codex() {
        let t = translator_for(Provider::Codex);
        let p = params("test", GuardConfig::from_preset(GuardPreset::Standard));
        let flags = t.build_flags(&p);
        assert!(flags.contains(&"-q".into()));
    }

    #[test]
    fn test_is_yolo_true() {
        let guards = GuardConfig::from_preset(GuardPreset::Yolo);
        assert!(is_yolo(&guards));
    }

    #[test]
    fn test_is_yolo_false_standard() {
        let guards = GuardConfig::from_preset(GuardPreset::Standard);
        assert!(!is_yolo(&guards));
    }

    #[test]
    fn test_is_yolo_false_strict() {
        let guards = GuardConfig::from_preset(GuardPreset::Strict);
        assert!(!is_yolo(&guards));
    }

    #[test]
    fn test_is_yolo_false_partial() {
        let mut guards = GuardConfig::from_preset(GuardPreset::Yolo);
        guards.git_push = false;
        assert!(!is_yolo(&guards));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("PATH", "PATH"));
        assert!(!glob_match("PATH", "HOME"));
    }

    #[test]
    fn test_glob_match_wildcard() {
        assert!(glob_match("AWS_*", "AWS_ACCESS_KEY_ID"));
        assert!(glob_match("AWS_*", "AWS_SECRET_ACCESS_KEY"));
        assert!(!glob_match("AWS_*", "PATH"));
    }

    #[test]
    fn test_glob_match_empty_prefix() {
        assert!(glob_match("*", "ANYTHING"));
    }

    #[test]
    fn test_filter_env_empty_filters() {
        let filter = EnvFilter::default();
        let mut env = HashMap::new();
        env.insert("A".into(), "1".into());
        let result = filter_env(&filter, env);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("A"));
    }

    #[test]
    fn test_filter_env_allow_only() {
        let filter = EnvFilter {
            allow: vec!["PATH".into(), "HOME".into()],
            deny: vec![],
        };
        let mut env = HashMap::new();
        env.insert("PATH".into(), "/usr/bin".into());
        env.insert("HOME".into(), "/home/user".into());
        env.insert("SECRET".into(), "hidden".into());
        let result = filter_env(&filter, env);
        assert_eq!(result.len(), 2);
        assert!(result.contains_key("PATH"));
        assert!(result.contains_key("HOME"));
    }

    #[test]
    fn test_filter_env_deny_only() {
        let filter = EnvFilter {
            allow: vec![],
            deny: vec!["AWS_*".into(), "SSH_*".into()],
        };
        let mut env = HashMap::new();
        env.insert("PATH".into(), "/usr/bin".into());
        env.insert("AWS_KEY".into(), "secret".into());
        env.insert("SSH_AUTH".into(), "sock".into());
        let result = filter_env(&filter, env);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("PATH"));
    }

    #[test]
    fn test_filter_env_allow_and_deny() {
        let filter = EnvFilter {
            allow: vec!["PATH".into(), "AWS_*".into()],
            deny: vec!["AWS_SECRET*".into()],
        };
        let mut env = HashMap::new();
        env.insert("PATH".into(), "/usr/bin".into());
        env.insert("AWS_KEY".into(), "key".into());
        env.insert("AWS_SECRET_KEY".into(), "secret".into());
        env.insert("HOME".into(), "/home".into());
        let result = filter_env(&filter, env);
        assert_eq!(result.len(), 2);
        assert!(result.contains_key("PATH"));
        assert!(result.contains_key("AWS_KEY"));
    }

    #[test]
    fn test_wrap_with_env_basic() {
        let mut env = HashMap::new();
        env.insert("PATH".into(), "/usr/bin".into());
        let result = wrap_with_env(&env, "claude");
        assert_eq!(result, "env -i PATH=/usr/bin claude");
    }

    #[test]
    fn test_wrap_with_env_sorted_keys() {
        let mut env = HashMap::new();
        env.insert("Z".into(), "1".into());
        env.insert("A".into(), "2".into());
        let result = wrap_with_env(&env, "cmd");
        assert_eq!(result, "env -i A=2 Z=1 cmd");
    }

    #[test]
    fn test_wrap_with_env_empty() {
        let env = HashMap::new();
        let result = wrap_with_env(&env, "claude");
        assert_eq!(result, "env -i claude");
    }

    #[test]
    fn test_claude_sanitize_env() {
        let t = ClaudeTranslator;
        let mut guards = GuardConfig::from_preset(GuardPreset::Standard);
        guards.env = EnvFilter {
            allow: vec!["PATH".into()],
            deny: vec![],
        };
        let mut env = HashMap::new();
        env.insert("PATH".into(), "/usr/bin".into());
        env.insert("SECRET".into(), "xyz".into());
        let filtered = t.sanitize_env(&guards, env);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_claude_translator_unrestricted_file_write() {
        let t = ClaudeTranslator;
        let mut guards = GuardConfig::from_preset(GuardPreset::Standard);
        guards.file_write = FileScope::Unrestricted;
        let p = params("test", guards);
        let flags = t.build_flags(&p);
        let tools_idx = flags.iter().position(|f| f == "--allowedTools").unwrap();
        let tools = &flags[tools_idx + 1];
        assert!(tools.contains("Edit"));
        assert!(tools.contains("Write"));
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
    }

    #[test]
    fn test_spawn_params_debug() {
        let p = SpawnParams {
            prompt: "test".into(),
            ..SpawnParams::default()
        };
        let debug = format!("{p:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_spawn_params_clone() {
        let p = SpawnParams {
            prompt: "test".into(),
            max_turns: Some(5),
            ..SpawnParams::default()
        };
        #[allow(clippy::redundant_clone)]
        let p2 = p.clone();
        assert_eq!(p2.prompt, "test");
        assert_eq!(p2.max_turns, Some(5));
    }
}
