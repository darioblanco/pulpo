pub mod repo;

use chrono::Utc;
use pulpo_common::knowledge::{Knowledge, KnowledgeKind};
use pulpo_common::session::Session;
use uuid::Uuid;

/// Maximum body length for extracted knowledge (characters).
const MAX_BODY_LEN: usize = 4000;

/// Error signal patterns in session output that indicate failure.
const ERROR_PATTERNS: &[&str] = &[
    "error:",
    "Error:",
    "ERROR:",
    "panic:",
    "PANIC:",
    "fatal:",
    "FATAL:",
    "OOM",
    "out of memory",
    "killed",
    "segfault",
    "SIGSEGV",
    "SIGKILL",
    "SIGABRT",
    "stack overflow",
    "timed out",
    "timeout",
];

/// Extract knowledge from a completed/dead session.
/// Returns zero or more `Knowledge` items (always a summary, plus a failure if errors detected).
pub fn extract(session: &Session) -> Vec<Knowledge> {
    let output = session.output_snapshot.as_deref().unwrap_or("");
    let mut results = Vec::new();

    // Always produce a summary
    results.push(build_summary(session, output));

    // Produce a failure record if error signals found
    if let Some(failure) = detect_failure(session, output) {
        results.push(failure);
    }

    results
}

fn build_summary(session: &Session, output: &str) -> Knowledge {
    let title = format!(
        "{} session \"{}\" on {}",
        status_label(session),
        session.name,
        session.provider,
    );

    let body = build_summary_body(session, output);

    let mut tags = vec![session.provider.to_string(), session.status.to_string()];
    if let Some(ink) = &session.ink {
        tags.push(format!("ink:{ink}"));
    }
    if let Some(model) = &session.model {
        tags.push(format!("model:{model}"));
    }

    Knowledge {
        id: Uuid::new_v4(),
        session_id: session.id,
        kind: KnowledgeKind::Summary,
        scope_repo: Some(session.workdir.clone()),
        scope_ink: session.ink.clone(),
        title,
        body,
        tags,
        relevance: summary_relevance(session, output),
        created_at: Utc::now(),
    }
}

fn build_summary_body(session: &Session, output: &str) -> String {
    use std::fmt::Write;

    let mut body = String::new();

    let _ = writeln!(body, "Prompt: {}", session.prompt);
    let _ = writeln!(body, "Provider: {}", session.provider);
    let _ = writeln!(body, "Status: {}", session.status);
    let _ = writeln!(body, "Workdir: {}", session.workdir);

    if let Some(model) = &session.model {
        let _ = writeln!(body, "Model: {model}");
    }
    if let Some(ink) = &session.ink {
        let _ = writeln!(body, "Ink: {ink}");
    }

    if !output.is_empty() {
        body.push_str("\n--- Output (tail) ---\n");
        body.push_str(&tail_output(output, MAX_BODY_LEN - body.len()));
    }

    truncate_to(&body, MAX_BODY_LEN)
}

fn detect_failure(session: &Session, output: &str) -> Option<Knowledge> {
    // Check intervention reason
    if let Some(reason) = &session.intervention_reason {
        return Some(build_failure(
            session,
            &format!("Intervention: {reason}"),
            output,
            0.9,
        ));
    }

    // Check exit code
    if let Some(code) = session.exit_code.filter(|&c| c != 0) {
        return Some(build_failure(
            session,
            &format!("Non-zero exit code: {code}"),
            output,
            0.7,
        ));
    }

    // Scan output for error patterns
    let output_lower = output.to_lowercase();
    for pattern in ERROR_PATTERNS {
        if output_lower.contains(&pattern.to_lowercase()) {
            return Some(build_failure(
                session,
                &format!("Error pattern detected: {pattern}"),
                output,
                0.6,
            ));
        }
    }

    None
}

fn build_failure(session: &Session, reason: &str, output: &str, relevance: f64) -> Knowledge {
    let title = format!(
        "Failure in \"{}\" on {}: {}",
        session.name, session.provider, reason,
    );

    let mut body = format!(
        "{reason}\n\nPrompt: {}\nWorkdir: {}\n",
        session.prompt, session.workdir
    );

    if !output.is_empty() {
        body.push_str("\n--- Output (tail) ---\n");
        body.push_str(&tail_output(output, MAX_BODY_LEN - body.len()));
    }

    let body = truncate_to(&body, MAX_BODY_LEN);

    let mut tags = vec![
        session.provider.to_string(),
        "failure".to_owned(),
        session.status.to_string(),
    ];
    if let Some(ink) = &session.ink {
        tags.push(format!("ink:{ink}"));
    }

    Knowledge {
        id: Uuid::new_v4(),
        session_id: session.id,
        kind: KnowledgeKind::Failure,
        scope_repo: Some(session.workdir.clone()),
        scope_ink: session.ink.clone(),
        title,
        body,
        tags,
        relevance,
        created_at: Utc::now(),
    }
}

const fn status_label(session: &Session) -> &'static str {
    match session.status {
        pulpo_common::session::SessionStatus::Completed => "Completed",
        pulpo_common::session::SessionStatus::Dead => "Dead",
        pulpo_common::session::SessionStatus::Stale => "Stale",
        _ => "Ended",
    }
}

fn summary_relevance(session: &Session, output: &str) -> f64 {
    let mut score = 0.5;

    // Longer output = probably more interesting
    if output.len() > 1000 {
        score += 0.1;
    }
    if output.len() > 5000 {
        score += 0.1;
    }

    // Sessions with an ink are more interesting (role-specific learning)
    if session.ink.is_some() {
        score += 0.1;
    }

    // Cap at 1.0
    if score > 1.0 {
        score = 1.0;
    }

    score
}

/// Take the last N characters of output, breaking at a line boundary if possible.
fn tail_output(output: &str, max_chars: usize) -> String {
    if output.len() <= max_chars {
        return output.to_owned();
    }
    let start = output.len() - max_chars;
    // Find next newline after start to break cleanly
    let break_point = output[start..]
        .find('\n')
        .map_or(start, |pos| start + pos + 1);
    format!("…{}", &output[break_point..])
}

/// Truncate a string to `max_len` characters.
fn truncate_to(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_owned()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::session::{Provider, SessionMode, SessionStatus};

    fn make_session(name: &str) -> Session {
        Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "Fix the auth bug".into(),
            status: SessionStatus::Dead,
            mode: SessionMode::Autonomous,
            conversation_id: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: Some("Running tests...\nAll 42 tests passed.\nDone.".into()),
            guard_config: None,
            model: Some("opus".into()),
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: Some("coder".into()),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_extract_produces_summary() {
        let session = make_session("my-task");
        let results = extract(&session);
        assert!(!results.is_empty());
        assert_eq!(results[0].kind, KnowledgeKind::Summary);
        assert!(results[0].title.contains("my-task"));
        assert!(results[0].title.contains("claude"));
    }

    #[test]
    fn test_extract_no_failure_for_clean_output() {
        let session = make_session("clean");
        let results = extract(&session);
        assert_eq!(results.len(), 1); // summary only
    }

    #[test]
    fn test_extract_failure_on_intervention() {
        let session = Session {
            intervention_reason: Some("Memory exceeded".into()),
            ..make_session("intervened")
        };
        let results = extract(&session);
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].kind, KnowledgeKind::Failure);
        assert!(results[1].title.contains("Intervention"));
        assert!((results[1].relevance - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn test_extract_failure_on_nonzero_exit() {
        let session = Session {
            exit_code: Some(1),
            ..make_session("exit-fail")
        };
        let results = extract(&session);
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].kind, KnowledgeKind::Failure);
        assert!(results[1].title.contains("Non-zero exit code"));
        assert!((results[1].relevance - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_extract_failure_on_error_pattern() {
        let session = Session {
            output_snapshot: Some("Building...\nError: compilation failed\nDone.".into()),
            ..make_session("error-out")
        };
        let results = extract(&session);
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].kind, KnowledgeKind::Failure);
        assert!(results[1].title.contains("Error pattern detected"));
    }

    #[test]
    fn test_extract_failure_oom_pattern() {
        let session = Session {
            output_snapshot: Some("Process killed: OOM".into()),
            ..make_session("oom")
        };
        let results = extract(&session);
        assert_eq!(results.len(), 2);
        assert!(results[1].title.contains("OOM"));
    }

    #[test]
    fn test_extract_no_output() {
        let session = Session {
            output_snapshot: None,
            ..make_session("no-output")
        };
        let results = extract(&session);
        assert_eq!(results.len(), 1); // summary only
        assert!(!results[0].body.contains("Output"));
    }

    #[test]
    fn test_extract_empty_output() {
        let session = Session {
            output_snapshot: Some(String::new()),
            ..make_session("empty-output")
        };
        let results = extract(&session);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_summary_has_scope() {
        let session = make_session("scoped");
        let results = extract(&session);
        assert_eq!(results[0].scope_repo.as_deref(), Some("/tmp/repo"));
        assert_eq!(results[0].scope_ink.as_deref(), Some("coder"));
    }

    #[test]
    fn test_summary_no_ink() {
        let session = Session {
            ink: None,
            ..make_session("no-ink")
        };
        let results = extract(&session);
        assert!(results[0].scope_ink.is_none());
        assert!(!results[0].tags.iter().any(|t| t.starts_with("ink:")));
    }

    #[test]
    fn test_summary_tags() {
        let session = make_session("tagged");
        let results = extract(&session);
        let tags = &results[0].tags;
        assert!(tags.contains(&"claude".to_owned()));
        assert!(tags.contains(&"dead".to_owned()));
        assert!(tags.contains(&"ink:coder".to_owned()));
        assert!(tags.contains(&"model:opus".to_owned()));
    }

    #[test]
    fn test_failure_tags() {
        let session = Session {
            intervention_reason: Some("OOM".into()),
            ..make_session("fail-tags")
        };
        let results = extract(&session);
        let tags = &results[1].tags;
        assert!(tags.contains(&"failure".to_owned()));
        assert!(tags.contains(&"claude".to_owned()));
        assert!(tags.contains(&"ink:coder".to_owned()));
    }

    #[test]
    fn test_summary_relevance_short_output() {
        let session = Session {
            output_snapshot: Some("short".into()),
            ink: None,
            ..make_session("short")
        };
        let results = extract(&session);
        assert!((results[0].relevance - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_summary_relevance_long_output() {
        let long_output = "x".repeat(5001);
        let session = Session {
            output_snapshot: Some(long_output),
            ..make_session("long")
        };
        let results = extract(&session);
        // 0.5 base + 0.1 (>1000) + 0.1 (>5000) + 0.1 (has ink) = 0.8
        assert!((results[0].relevance - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_summary_relevance_with_ink() {
        let session = make_session("inked");
        let results = extract(&session);
        // 0.5 base + 0.1 (ink) = 0.6
        assert!((results[0].relevance - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_status_label_completed() {
        let session = Session {
            status: SessionStatus::Completed,
            ..make_session("completed")
        };
        let results = extract(&session);
        assert!(results[0].title.starts_with("Completed"));
    }

    #[test]
    fn test_status_label_dead() {
        let session = make_session("dead");
        let results = extract(&session);
        assert!(results[0].title.starts_with("Dead"));
    }

    #[test]
    fn test_status_label_stale() {
        let session = Session {
            status: SessionStatus::Stale,
            ..make_session("stale")
        };
        let results = extract(&session);
        assert!(results[0].title.starts_with("Stale"));
    }

    #[test]
    fn test_status_label_running() {
        let session = Session {
            status: SessionStatus::Running,
            ..make_session("running")
        };
        let results = extract(&session);
        assert!(results[0].title.starts_with("Ended"));
    }

    #[test]
    fn test_tail_output_short() {
        let result = tail_output("short text", 100);
        assert_eq!(result, "short text");
    }

    #[test]
    fn test_tail_output_truncates() {
        let output = "line1\nline2\nline3\nline4\nline5";
        let result = tail_output(output, 15);
        assert!(result.starts_with('…'));
        assert!(result.len() <= 16); // 15 + "…"
    }

    #[test]
    fn test_truncate_to_short() {
        assert_eq!(truncate_to("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_to_exact() {
        assert_eq!(truncate_to("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_to_long() {
        let result = truncate_to("hello world", 8);
        assert_eq!(result, "hello w…");
        assert_eq!(result.len(), 10); // "…" is 3 bytes in UTF-8
    }

    #[test]
    fn test_intervention_takes_priority_over_exit_code() {
        let session = Session {
            intervention_reason: Some("Memory exceeded".into()),
            exit_code: Some(1),
            output_snapshot: Some("Error: something bad".into()),
            ..make_session("priority")
        };
        let results = extract(&session);
        // Only 1 failure (intervention), not 3
        assert_eq!(results.len(), 2);
        assert!(results[1].title.contains("Intervention"));
    }

    #[test]
    fn test_exit_code_zero_no_failure() {
        let session = Session {
            exit_code: Some(0),
            ..make_session("clean-exit")
        };
        let results = extract(&session);
        assert_eq!(results.len(), 1); // summary only
    }

    #[test]
    fn test_summary_body_includes_prompt() {
        let session = make_session("body-test");
        let results = extract(&session);
        assert!(results[0].body.contains("Fix the auth bug"));
    }

    #[test]
    fn test_summary_body_includes_model() {
        let session = make_session("model-body");
        let results = extract(&session);
        assert!(results[0].body.contains("Model: opus"));
    }

    #[test]
    fn test_summary_body_no_model() {
        let session = Session {
            model: None,
            ..make_session("no-model")
        };
        let results = extract(&session);
        assert!(!results[0].body.contains("Model:"));
    }

    #[test]
    fn test_failure_body_includes_output() {
        let session = Session {
            exit_code: Some(1),
            output_snapshot: Some("Error: test failed".into()),
            ..make_session("fail-body")
        };
        let results = extract(&session);
        assert!(results[1].body.contains("Error: test failed"));
    }

    #[test]
    fn test_extract_case_insensitive_error_detection() {
        let session = Session {
            output_snapshot: Some("fatal: something went wrong".into()),
            ..make_session("case")
        };
        let results = extract(&session);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_extract_all_error_patterns() {
        for pattern in ERROR_PATTERNS {
            let session = Session {
                output_snapshot: Some(format!("Some output with {pattern} in it")),
                ..make_session("pattern-test")
            };
            let results = extract(&session);
            assert!(
                results.len() >= 2,
                "Pattern '{pattern}' should trigger failure detection"
            );
        }
    }

    #[test]
    fn test_different_providers() {
        for provider in [
            Provider::Claude,
            Provider::Codex,
            Provider::Gemini,
            Provider::OpenCode,
        ] {
            let session = Session {
                provider,
                ..make_session("provider-test")
            };
            let results = extract(&session);
            assert!(results[0].tags.contains(&provider.to_string()));
        }
    }

    #[test]
    fn test_knowledge_ids_are_unique() {
        let session = Session {
            intervention_reason: Some("fail".into()),
            ..make_session("unique-ids")
        };
        let results = extract(&session);
        assert_ne!(results[0].id, results[1].id);
    }

    #[test]
    fn test_knowledge_session_id_matches() {
        let session = make_session("id-match");
        let results = extract(&session);
        assert_eq!(results[0].session_id, session.id);
    }

    #[test]
    fn test_summary_body_includes_ink() {
        let session = make_session("ink-body");
        let results = extract(&session);
        assert!(results[0].body.contains("Ink: coder"));
    }

    #[test]
    fn test_failure_no_ink_in_tags() {
        let session = Session {
            ink: None,
            exit_code: Some(1),
            ..make_session("no-ink-fail")
        };
        let results = extract(&session);
        assert!(!results[1].tags.iter().any(|t| t.starts_with("ink:")));
    }

    #[test]
    fn test_extract_very_long_output_truncated() {
        let long_output = "x\n".repeat(10000);
        let session = Session {
            output_snapshot: Some(long_output),
            ..make_session("long-output")
        };
        let results = extract(&session);
        assert!(results[0].body.len() <= MAX_BODY_LEN + 3); // +3 for "…" UTF-8
    }

    #[test]
    fn test_error_patterns_constant() {
        // Verify the constant is non-empty and all patterns are lowercase-matchable
        assert!(!ERROR_PATTERNS.is_empty());
        for pattern in ERROR_PATTERNS {
            assert!(!pattern.is_empty());
        }
    }

    #[test]
    fn test_max_body_len_constant() {
        const { assert!(MAX_BODY_LEN > 0) };
        const { assert!(MAX_BODY_LEN <= 10000) };
    }

    #[test]
    fn test_summary_no_model_no_ink_tags() {
        let session = Session {
            model: None,
            ink: None,
            ..make_session("minimal-tags")
        };
        let results = extract(&session);
        assert!(!results[0].tags.iter().any(|t| t.starts_with("model:")));
        assert!(!results[0].tags.iter().any(|t| t.starts_with("ink:")));
    }
}
