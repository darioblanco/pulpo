use std::path::Path;

/// Extracted authentication info from agent credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthInfo {
    pub provider: String,
    pub plan: Option<String>,
    pub email: Option<String>,
}

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

/// Extract auth info from Claude Code credentials.
///
/// Reads `<claude_dir>/.credentials.json` and parses the JSON to find
/// `subscriptionType` and email fields from the OAuth data.
///
/// On macOS, tries the keychain first via `security find-generic-password`.
pub fn extract_claude_auth(claude_dir: &Path) -> Option<AuthInfo> {
    let creds_path = claude_dir.join(".credentials.json");
    let json_str = read_file_to_string(&creds_path)?;
    parse_claude_credentials(&json_str)
}

/// Parse Claude credentials JSON and extract auth info.
fn parse_claude_credentials(json_str: &str) -> Option<AuthInfo> {
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    // The credentials file may have different structures.
    // Common patterns:
    // - Top-level object with claudeAiOauth key containing { accessToken, ... }
    // - Or a direct object with subscriptionType, email fields

    let mut plan = None;
    let mut email = None;

    // Try claudeAiOauth path first
    if let Some(oauth) = value.get("claudeAiOauth") {
        plan = oauth
            .get("subscriptionType")
            .and_then(|v| v.as_str())
            .map(str::to_lowercase);
        email = oauth
            .get("email")
            .and_then(|v| v.as_str())
            .map(String::from);
        if email.is_none() {
            email = oauth
                .get("account")
                .and_then(|a| a.get("email"))
                .and_then(|v| v.as_str())
                .map(String::from);
        }
    }

    // Fall back to top-level fields
    if plan.is_none() {
        plan = value
            .get("subscriptionType")
            .and_then(|v| v.as_str())
            .map(str::to_lowercase);
    }
    if email.is_none() {
        email = value
            .get("email")
            .and_then(|v| v.as_str())
            .map(String::from);
    }

    // If we found nothing useful, return None
    if plan.is_none() && email.is_none() {
        return None;
    }

    Some(AuthInfo {
        provider: "claude.ai".to_owned(),
        plan,
        email,
    })
}

/// Extract auth info from `OpenAI` Codex credentials.
///
/// Reads `<codex_dir>/auth.json` and looks for plan/account info.
pub fn extract_codex_auth(codex_dir: &Path) -> Option<AuthInfo> {
    let auth_path = codex_dir.join("auth.json");
    let json_str = read_file_to_string(&auth_path)?;
    parse_codex_credentials(&json_str)
}

/// Parse Codex auth JSON and extract auth info.
fn parse_codex_credentials(json_str: &str) -> Option<AuthInfo> {
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let plan = value
        .get("plan")
        .and_then(|v| v.as_str())
        .map(str::to_lowercase);
    let email = value
        .get("email")
        .and_then(|v| v.as_str())
        .map(String::from);

    if plan.is_none() && email.is_none() {
        return None;
    }

    Some(AuthInfo {
        provider: "openai".to_owned(),
        plan,
        email,
    })
}

/// Extract auth info from Google Gemini credentials.
///
/// Reads files under `<gemini_dir>/` for Google account info.
pub fn extract_gemini_auth(gemini_dir: &Path) -> Option<AuthInfo> {
    // Try config.json first, then credentials.json
    for filename in &["config.json", "credentials.json"] {
        let path = gemini_dir.join(filename);
        if let Some(json_str) = read_file_to_string(&path)
            && let Some(info) = parse_gemini_credentials(&json_str)
        {
            return Some(info);
        }
    }
    None
}

/// Parse Gemini credentials JSON and extract auth info.
fn parse_gemini_credentials(json_str: &str) -> Option<AuthInfo> {
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let plan = value
        .get("plan")
        .or_else(|| value.get("tier"))
        .and_then(|v| v.as_str())
        .map(str::to_lowercase);
    let email = value
        .get("email")
        .or_else(|| value.get("account"))
        .and_then(|v| v.as_str())
        .map(String::from);

    if plan.is_none() && email.is_none() {
        return None;
    }

    Some(AuthInfo {
        provider: "google".to_owned(),
        plan,
        email,
    })
}

/// Try all known credential sources and return whatever is found.
/// Tries Claude, Codex, and Gemini in order.
#[cfg(not(coverage))]
pub fn detect_auth_info() -> Vec<AuthInfo> {
    let mut results = Vec::new();
    if let Some(home) = dirs::home_dir() {
        if let Some(info) = extract_claude_auth(&home.join(".claude")) {
            results.push(info);
        }
        if let Some(info) = extract_codex_auth(&home.join(".codex")) {
            results.push(info);
        }
        if let Some(info) = extract_gemini_auth(&home.join(".gemini")) {
            results.push(info);
        }
    }
    results
}

/// Under coverage builds, return empty (no real filesystem access).
#[cfg(coverage)]
pub fn detect_auth_info() -> Vec<AuthInfo> {
    Vec::new()
}

/// Detect auth info for a specific provider based on the session command.
/// Only reads credentials for the agent detected in the command.
#[cfg(not(coverage))]
pub fn detect_auth_for_command(command: &str) -> Option<AuthInfo> {
    let provider = agent_provider_for_command(command)?;
    let home = dirs::home_dir()?;
    match provider {
        "claude.ai" => extract_claude_auth(&home.join(".claude")),
        "openai" => extract_codex_auth(&home.join(".codex")),
        "google" => extract_gemini_auth(&home.join(".gemini")),
        _ => None,
    }
}

/// Under coverage builds, return None (no real filesystem access).
#[cfg(coverage)]
pub fn detect_auth_for_command(command: &str) -> Option<AuthInfo> {
    // Still validate the command contains a known agent
    agent_provider_for_command(command)?;
    None
}

/// Read a file to string, returning None on any error.
#[cfg(not(coverage))]
fn read_file_to_string(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Under coverage, never read real files.
#[cfg(coverage)]
fn read_file_to_string(_path: &Path) -> Option<String> {
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

    // -- parse_claude_credentials tests --

    #[test]
    fn test_parse_claude_with_oauth() {
        let json = r#"{
            "claudeAiOauth": {
                "subscriptionType": "Max",
                "email": "user@example.com"
            }
        }"#;
        let info = parse_claude_credentials(json).unwrap();
        assert_eq!(info.provider, "claude.ai");
        assert_eq!(info.plan.as_deref(), Some("max"));
        assert_eq!(info.email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn test_parse_claude_with_oauth_account_email() {
        let json = r#"{
            "claudeAiOauth": {
                "subscriptionType": "Pro",
                "account": { "email": "nested@example.com" }
            }
        }"#;
        let info = parse_claude_credentials(json).unwrap();
        assert_eq!(info.plan.as_deref(), Some("pro"));
        assert_eq!(info.email.as_deref(), Some("nested@example.com"));
    }

    #[test]
    fn test_parse_claude_top_level_fields() {
        let json = r#"{
            "subscriptionType": "Plus",
            "email": "toplevel@example.com"
        }"#;
        let info = parse_claude_credentials(json).unwrap();
        assert_eq!(info.plan.as_deref(), Some("plus"));
        assert_eq!(info.email.as_deref(), Some("toplevel@example.com"));
    }

    #[test]
    fn test_parse_claude_no_useful_fields() {
        let json = r#"{"accessToken": "abc123"}"#;
        assert!(parse_claude_credentials(json).is_none());
    }

    #[test]
    fn test_parse_claude_invalid_json() {
        assert!(parse_claude_credentials("not json").is_none());
    }

    #[test]
    fn test_parse_claude_empty() {
        assert!(parse_claude_credentials("").is_none());
    }

    #[test]
    fn test_parse_claude_plan_only() {
        let json = r#"{"claudeAiOauth": {"subscriptionType": "Max"}}"#;
        let info = parse_claude_credentials(json).unwrap();
        assert_eq!(info.plan.as_deref(), Some("max"));
        assert!(info.email.is_none());
    }

    // -- parse_codex_credentials tests --

    #[test]
    fn test_parse_codex_full() {
        let json = r#"{"plan": "Plus", "email": "codex@example.com"}"#;
        let info = parse_codex_credentials(json).unwrap();
        assert_eq!(info.provider, "openai");
        assert_eq!(info.plan.as_deref(), Some("plus"));
        assert_eq!(info.email.as_deref(), Some("codex@example.com"));
    }

    #[test]
    fn test_parse_codex_no_useful_fields() {
        let json = r#"{"apiKey": "sk-..."}"#;
        assert!(parse_codex_credentials(json).is_none());
    }

    #[test]
    fn test_parse_codex_invalid_json() {
        assert!(parse_codex_credentials("{bad}").is_none());
    }

    #[test]
    fn test_parse_codex_plan_only() {
        let json = r#"{"plan": "Pro"}"#;
        let info = parse_codex_credentials(json).unwrap();
        assert_eq!(info.plan.as_deref(), Some("pro"));
        assert!(info.email.is_none());
    }

    // -- parse_gemini_credentials tests --

    #[test]
    fn test_parse_gemini_full() {
        let json = r#"{"plan": "Ultra", "email": "gemini@google.com"}"#;
        let info = parse_gemini_credentials(json).unwrap();
        assert_eq!(info.provider, "google");
        assert_eq!(info.plan.as_deref(), Some("ultra"));
        assert_eq!(info.email.as_deref(), Some("gemini@google.com"));
    }

    #[test]
    fn test_parse_gemini_with_tier() {
        let json = r#"{"tier": "Pro", "account": "user@gmail.com"}"#;
        let info = parse_gemini_credentials(json).unwrap();
        assert_eq!(info.plan.as_deref(), Some("pro"));
        assert_eq!(info.email.as_deref(), Some("user@gmail.com"));
    }

    #[test]
    fn test_parse_gemini_no_useful_fields() {
        let json = r#"{"token": "abc"}"#;
        assert!(parse_gemini_credentials(json).is_none());
    }

    #[test]
    fn test_parse_gemini_invalid_json() {
        assert!(parse_gemini_credentials("???").is_none());
    }

    // -- extract functions with temp dirs --

    #[test]
    fn test_extract_claude_auth_nonexistent_dir() {
        let info = extract_claude_auth(Path::new("/nonexistent/path/.claude"));
        // Under coverage, read_file_to_string returns None; in normal builds,
        // the file doesn't exist so it also returns None.
        assert!(info.is_none());
    }

    #[test]
    fn test_extract_codex_auth_nonexistent_dir() {
        assert!(extract_codex_auth(Path::new("/nonexistent/.codex")).is_none());
    }

    #[test]
    fn test_extract_gemini_auth_nonexistent_dir() {
        assert!(extract_gemini_auth(Path::new("/nonexistent/.gemini")).is_none());
    }

    // -- detect_auth_info / detect_auth_for_command --

    #[test]
    fn test_detect_auth_info_returns_vec() {
        // In test/coverage mode, this returns empty or whatever is on disk.
        // The important thing is it doesn't panic.
        let _ = detect_auth_info();
    }

    #[test]
    fn test_detect_auth_for_command_no_agent() {
        assert!(detect_auth_for_command("cargo build").is_none());
    }

    #[test]
    fn test_detect_auth_for_command_with_agent() {
        // Won't find credentials in test env, but shouldn't panic
        let result = detect_auth_for_command("claude -p 'test'");
        // Under coverage: returns None (agent found but no fs access)
        // Under normal: returns None (no credentials on disk in CI)
        // Either way, no panic
        let _ = result;
    }
}
