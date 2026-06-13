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

/// Classify a command's billing pool by whether it runs headless (`-p` / `--print`).
pub fn detect_pool(command: &str) -> &'static str {
    let headless = command
        .split_whitespace()
        .any(|tok| tok == "-p" || tok == "--print" || tok.starts_with("--print="));
    if headless {
        POOL_HEADLESS
    } else {
        POOL_SUBSCRIPTION
    }
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
}
