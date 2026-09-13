//! Agent-name detection for a session's command.
//!
//! `agent_provider_for_command` maps a session's launch command to the provider whose
//! structured usage reader should read its session files (see `usage::read_exact_usage`).
//! This is the only surviving piece of what used to be a broader credential/plan
//! extraction module: reading an agent's local credentials file (and, per an earlier
//! doc comment here, the macOS keychain) to attribute sessions to a billing pool and
//! show an "auth state" badge in the web UI was removed — a metering tool has no
//! business reading account credentials off disk, and the pool/plan attribution it fed
//! (`usage::pool`, `usage::projection`, the `[plans]` config) was removed with it.

/// Known agent command names mapped to their credential extractors.
const KNOWN_AGENTS: &[(&str, &str)] = &[
    ("claude", "claude.ai"),
    ("codex", "openai"),
    ("gemini", "google"),
];

/// Check if a command string contains a known agent name.
/// Returns the provider string if found.
pub fn agent_provider_for_command(command: &str) -> Option<&'static str> {
    let lower = command.to_lowercase();
    for &(agent, provider) in KNOWN_AGENTS {
        // Match the agent name as a word boundary: at start, after whitespace, or after /
        for (i, _) in lower.match_indices(agent) {
            let before_ok = i == 0
                || lower.as_bytes().get(i - 1).is_some_and(|&b| {
                    b == b' ' || b == b'/' || b == b'\t' || b == b'\n' || b == b';' || b == b'&'
                });
            let after = i + agent.len();
            let after_ok = after >= lower.len()
                || lower.as_bytes().get(after).is_some_and(|&b| {
                    b == b' ' || b == b'\t' || b == b'\n' || b == b';' || b == b'&'
                });
            if before_ok && after_ok {
                return Some(provider);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- agent_provider_for_command tests --

    #[test]
    fn test_detects_claude_command() {
        assert_eq!(
            agent_provider_for_command("claude -p 'review code'"),
            Some("claude.ai")
        );
    }

    #[test]
    fn test_detects_claude_with_path() {
        assert_eq!(
            agent_provider_for_command("/usr/local/bin/claude --help"),
            Some("claude.ai")
        );
    }

    #[test]
    fn test_detects_codex_command() {
        assert_eq!(
            agent_provider_for_command("codex run tests"),
            Some("openai")
        );
    }

    #[test]
    fn test_detects_gemini_command() {
        assert_eq!(agent_provider_for_command("gemini chat"), Some("google"));
    }

    #[test]
    fn test_no_agent_in_command() {
        assert_eq!(agent_provider_for_command("cargo test --workspace"), None);
    }

    #[test]
    fn test_no_partial_match() {
        // "claudette" should not match "claude"
        assert_eq!(agent_provider_for_command("claudette run"), None);
    }

    #[test]
    fn test_agent_after_semicolon() {
        assert_eq!(
            agent_provider_for_command("cd /repo; claude -p 'fix'"),
            Some("claude.ai")
        );
    }

    #[test]
    fn test_agent_after_ampersand() {
        assert_eq!(
            agent_provider_for_command("export FOO=1 && codex run"),
            Some("openai")
        );
    }

    #[test]
    fn test_agent_at_end_of_command() {
        assert_eq!(agent_provider_for_command("exec claude"), Some("claude.ai"));
    }

    #[test]
    fn test_empty_command() {
        assert_eq!(agent_provider_for_command(""), None);
    }

    #[test]
    fn test_no_match_for_substring_codex() {
        // "mycodex" should not match "codex"
        assert_eq!(agent_provider_for_command("mycodex run"), None);
    }

    #[test]
    fn test_agent_with_pipe() {
        // "claude" appears after a pipe — the | is not in the boundary chars,
        // but it's preceded by space: "| claude"
        assert_eq!(
            agent_provider_for_command("echo foo | claude -p 'fix'"),
            Some("claude.ai")
        );
    }

    #[test]
    fn test_agent_case_insensitive() {
        assert_eq!(
            agent_provider_for_command("Claude -p 'test'"),
            Some("claude.ai")
        );
        assert_eq!(agent_provider_for_command("CODEX run"), Some("openai"));
        assert_eq!(agent_provider_for_command("GEMINI chat"), Some("google"));
    }

    #[test]
    fn test_agent_after_tab() {
        assert_eq!(
            agent_provider_for_command("cd /repo\tclaude -p 'fix'"),
            Some("claude.ai")
        );
    }

    #[test]
    fn test_agent_after_newline() {
        assert_eq!(
            agent_provider_for_command("export FOO=1\nclaude -p 'fix'"),
            Some("claude.ai")
        );
    }

    #[test]
    fn test_agent_gemini_at_end() {
        assert_eq!(agent_provider_for_command("exec gemini"), Some("google"));
    }

    #[test]
    fn test_no_match_agent_in_middle_of_word() {
        // "geminist" should not match "gemini"
        assert_eq!(agent_provider_for_command("geminist run"), None);
    }
}
