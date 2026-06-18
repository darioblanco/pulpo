//! Billing-pool attribution from a session's command.
//!
//! Anthropic split usage into two pools (effective June 15, 2026): interactive sessions
//! draw from the **subscription** pool, while headless `claude -p` / Agent SDK usage draws
//! from a separate monthly **headless** credit pool. Pulpo runs agents in tmux under a real
//! TTY, so its sessions are interactive and stay on the subscription pool — unlike
//! SDK-built orchestrators. We surface which pool a session draws from so the projection
//! rollups keep the two kinds of headroom separate.
//!
//! The distinction is most meaningful for Claude; for other agents the label still reflects
//! whether the command runs headless, which is the billing-relevant signal.

/// Subscription pool — interactive session under a TTY (Pulpo's default).
pub const POOL_SUBSCRIPTION: &str = "subscription";
/// Headless credit pool — `-p` / `--print` (non-interactive) usage.
pub const POOL_HEADLESS: &str = "headless";

/// Classify a command's billing pool by whether it runs headless.
///
/// Headless markers: Claude's `-p` / `--print`, or Codex's non-interactive `exec`
/// subcommand (`codex exec …`). Anything else is treated as an interactive session
/// on the subscription pool.
pub fn detect_pool(command: &str) -> &'static str {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let claude_headless = tokens
        .iter()
        .any(|tok| *tok == "-p" || *tok == "--print" || tok.starts_with("--print="));
    if claude_headless || is_codex_exec(&tokens) {
        POOL_HEADLESS
    } else {
        POOL_SUBSCRIPTION
    }
}

/// True when the command invokes Codex with its `exec` subcommand (`codex exec …`),
/// the non-interactive mode that draws from the headless pool. Matches the `codex`
/// executable (with optional path prefix) followed by `exec` as its first non-flag token.
fn is_codex_exec(tokens: &[&str]) -> bool {
    for (i, tok) in tokens.iter().enumerate() {
        let base = tok.rsplit('/').next().unwrap_or(tok);
        if base == "codex" {
            return tokens[i + 1..]
                .iter()
                .find(|t| !t.starts_with('-'))
                .is_some_and(|t| *t == "exec");
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interactive_is_subscription() {
        assert_eq!(detect_pool("claude"), POOL_SUBSCRIPTION);
        assert_eq!(detect_pool("claude 'fix the bug'"), POOL_SUBSCRIPTION);
        assert_eq!(detect_pool("codex"), POOL_SUBSCRIPTION);
    }

    #[test]
    fn test_print_flag_is_headless() {
        assert_eq!(detect_pool("claude -p 'review'"), POOL_HEADLESS);
        assert_eq!(detect_pool("claude --print 'review'"), POOL_HEADLESS);
        assert_eq!(detect_pool("claude --print='review'"), POOL_HEADLESS);
    }

    #[test]
    fn test_print_flag_with_leading_env_and_path() {
        assert_eq!(
            detect_pool("cd /repo && /usr/local/bin/claude -p 'go'"),
            POOL_HEADLESS
        );
    }

    #[test]
    fn test_no_false_positive_on_substring() {
        // a prompt that contains "-p" inside a word must not trip detection
        assert_eq!(detect_pool("claude 'add a -ping flag'"), POOL_SUBSCRIPTION);
        assert_eq!(detect_pool("claude 'use --printer'"), POOL_SUBSCRIPTION);
    }

    #[test]
    fn test_empty_command() {
        assert_eq!(detect_pool(""), POOL_SUBSCRIPTION);
    }

    #[test]
    fn test_codex_exec_is_headless() {
        assert_eq!(detect_pool("codex exec 'fix it'"), POOL_HEADLESS);
        assert_eq!(detect_pool("/opt/bin/codex exec 'fix'"), POOL_HEADLESS);
        // flags between `codex` and `exec` are skipped to the first non-flag token
        assert_eq!(detect_pool("codex --json exec 'fix'"), POOL_HEADLESS);
    }

    #[test]
    fn test_codex_interactive_and_non_exec_subcommands_are_subscription() {
        assert_eq!(detect_pool("codex"), POOL_SUBSCRIPTION);
        assert_eq!(detect_pool("codex 'just chat'"), POOL_SUBSCRIPTION);
        // a different subcommand, or `exec` only as a prompt word, must not trip it
        assert_eq!(detect_pool("codex login"), POOL_SUBSCRIPTION);
        assert_eq!(detect_pool("claude 'run exec later'"), POOL_SUBSCRIPTION);
    }
}
