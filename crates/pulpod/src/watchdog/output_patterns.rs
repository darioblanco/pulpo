/// Extract a PR/MR URL from terminal output.
///
/// Checks for:
/// - GitHub PR URLs: `https://github.com/{owner}/{repo}/pull/{number}`
/// - GitLab MR URLs: `https://gitlab.com/{path}/-/merge_requests/{number}`
/// - Bitbucket PR URLs: `https://bitbucket.org/{owner}/{repo}/pull-requests/{number}`
///
/// Returns the first match found, or `None`.
pub fn extract_pr_url(output: &str) -> Option<String> {
    // Strip ANSI escape codes for reliable matching
    let cleaned = strip_ansi(output);

    for line in cleaned.lines() {
        // GitHub PR
        if let Some(url) = find_url_with_path(line, "github.com", "/pull/") {
            return Some(url);
        }
        // GitLab MR
        if let Some(url) = find_url_with_path(line, "gitlab.com", "/-/merge_requests/") {
            return Some(url);
        }
        // Bitbucket PR
        if let Some(url) = find_url_with_path(line, "bitbucket.org", "/pull-requests/") {
            return Some(url);
        }
    }
    None
}

/// Extract a branch name from git push output.
///
/// Checks for:
/// - `remote: Create a pull request for 'branch-name'` (GitHub push output)
/// - `* [new branch]      branch-name -> branch-name` (git push output)
/// - `Branch 'branch-name' set up to track` (git push output)
///
/// Returns the first match found, or `None`.
pub fn extract_branch(output: &str) -> Option<String> {
    let cleaned = strip_ansi(output);

    for line in cleaned.lines() {
        let trimmed = line.trim();

        // Pattern: remote: Create a pull request for 'branch-name'
        if let Some(rest) = trimmed.strip_prefix("remote:") {
            let rest = rest.trim();
            let needle = "Create a pull request for '";
            if let Some(after) = rest.find(needle).map(|i| &rest[i + needle.len()..])
                && let Some(end) = after.find('\'')
            {
                let branch = &after[..end];
                if !branch.is_empty() {
                    return Some(branch.to_owned());
                }
            }
        }

        // Pattern: * [new branch]      branch-name -> branch-name
        if let Some(rest) = trimmed.strip_prefix("* [new branch]") {
            let rest = rest.trim();
            if let Some(arrow) = rest.find("->") {
                let branch = rest[..arrow].trim();
                if !branch.is_empty() {
                    return Some(branch.to_owned());
                }
            }
        }

        // Pattern: Branch 'branch-name' set up to track
        let needle = "Branch '";
        if let Some(after) = trimmed.find(needle).map(|i| &trimmed[i + needle.len()..])
            && let Some(end) = after.find('\'')
        {
            let rest_after_quote = &after[end..];
            if rest_after_quote.contains("set up to track") {
                let branch = &after[..end];
                if !branch.is_empty() {
                    return Some(branch.to_owned());
                }
            }
        }
    }
    None
}

/// Strip ANSI escape codes from a string.
fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip ESC [ ... final_byte sequence
            if let Some(next) = chars.next()
                && next == '['
            {
                // CSI sequence: consume until 0x40-0x7E
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            // else: skip the single char after ESC (e.g., ESC])
        } else {
            result.push(ch);
        }
    }
    result
}

/// Find a URL in a line that contains the given host and path segment.
/// Extracts from `https://` up to the first whitespace or end of line.
fn find_url_with_path(line: &str, host: &str, path_segment: &str) -> Option<String> {
    let prefix = format!("https://{host}");
    let mut search_from = 0;
    while search_from < line.len() {
        let haystack = &line[search_from..];
        let Some(start) = haystack.find(&prefix) else {
            break;
        };
        let abs_start = search_from + start;
        // Find end of URL (whitespace, end of line, or common delimiters)
        let url_start = &line[abs_start..];
        let end = url_start
            .find(|c: char| c.is_whitespace() || c == '"' || c == '>' || c == ')' || c == ']')
            .unwrap_or(url_start.len());
        let url = &url_start[..end];
        if url.contains(path_segment) {
            return Some(url.to_owned());
        }
        search_from = abs_start + end;
    }
    None
}

/// Patterns that indicate rate limiting in agent output.
/// Each entry is (pattern, human-readable label).
const RATE_LIMIT_PATTERNS: &[(&str, &str)] = &[
    ("rate limit", "Rate limited"),
    ("too many requests", "Rate limited: too many requests"),
    ("429", "Rate limited (429)"),
    ("capacity", "Rate limited: at capacity"),
    ("overloaded", "Rate limited: overloaded"),
    ("quota exceeded", "Rate limited: quota exceeded"),
    ("resource_exhausted", "Rate limited: resource exhausted"),
];

/// Detect rate limit warnings in agent output.
/// Returns a human-readable message if rate limiting is detected.
///
/// Checks the last 20 lines of output (after ANSI stripping) for known
/// rate limit patterns. Returns the first match found.
pub fn detect_rate_limit(output: &str) -> Option<String> {
    let cleaned = strip_ansi(output);
    let last_lines: Vec<&str> = cleaned.lines().rev().take(20).collect();

    for line in &last_lines {
        let lower = line.to_lowercase();
        for &(pattern, label) in RATE_LIMIT_PATTERNS {
            if lower.contains(pattern) {
                return Some(label.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- extract_pr_url tests --

    #[test]
    fn test_extract_github_pr_url() {
        let output = "remote: \nremote: Create a pull request:\nremote:   https://github.com/owner/repo/pull/42\nremote: \n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://github.com/owner/repo/pull/42".into())
        );
    }

    #[test]
    fn test_extract_gitlab_mr_url() {
        let output = "View merge request: https://gitlab.com/group/project/-/merge_requests/123\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://gitlab.com/group/project/-/merge_requests/123".into())
        );
    }

    #[test]
    fn test_extract_bitbucket_pr_url() {
        let output = "Create pull request: https://bitbucket.org/owner/repo/pull-requests/99\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://bitbucket.org/owner/repo/pull-requests/99".into())
        );
    }

    #[test]
    fn test_extract_pr_url_no_match() {
        let output = "$ cargo test\nrunning 42 tests\nall passed\n";
        assert_eq!(extract_pr_url(output), None);
    }

    #[test]
    fn test_extract_pr_url_multiple_returns_first() {
        let output =
            "First: https://github.com/a/b/pull/1\nSecond: https://github.com/c/d/pull/2\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://github.com/a/b/pull/1".into())
        );
    }

    #[test]
    fn test_extract_pr_url_with_ansi_codes() {
        let output = "\x1b[32mhttps://github.com/owner/repo/pull/42\x1b[0m\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://github.com/owner/repo/pull/42".into())
        );
    }

    #[test]
    fn test_extract_pr_url_github_not_pr_path() {
        let output = "See https://github.com/owner/repo/issues/42\n";
        assert_eq!(extract_pr_url(output), None);
    }

    #[test]
    fn test_extract_pr_url_gh_create_output() {
        // Real `gh pr create` output
        let output = "Creating pull request for feature-branch into main in owner/repo\n\nhttps://github.com/owner/repo/pull/7\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://github.com/owner/repo/pull/7".into())
        );
    }

    #[test]
    fn test_extract_pr_url_nested_gitlab_path() {
        let output = "https://gitlab.com/group/sub/project/-/merge_requests/5\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://gitlab.com/group/sub/project/-/merge_requests/5".into())
        );
    }

    // -- extract_branch tests --

    #[test]
    fn test_extract_branch_github_remote_message() {
        let output = "remote: \nremote: Create a pull request for 'feature/my-branch' on GitHub by visiting:\nremote:   https://github.com/owner/repo/pull/new/feature/my-branch\nremote: \n";
        assert_eq!(extract_branch(output), Some("feature/my-branch".into()));
    }

    #[test]
    fn test_extract_branch_new_branch_output() {
        let output = "To github.com:owner/repo.git\n * [new branch]      my-branch -> my-branch\n";
        assert_eq!(extract_branch(output), Some("my-branch".into()));
    }

    #[test]
    fn test_extract_branch_set_up_to_track() {
        let output = "Branch 'fix-bug' set up to track remote branch 'fix-bug' from 'origin'.\n";
        assert_eq!(extract_branch(output), Some("fix-bug".into()));
    }

    #[test]
    fn test_extract_branch_no_match() {
        let output = "$ cargo build\nCompiling pulpo v0.1.0\nFinished dev\n";
        assert_eq!(extract_branch(output), None);
    }

    #[test]
    fn test_extract_branch_with_ansi_codes() {
        let output = "\x1b[33mremote: Create a pull request for 'ansi-branch' on GitHub\x1b[0m\n";
        assert_eq!(extract_branch(output), Some("ansi-branch".into()));
    }

    #[test]
    fn test_extract_branch_multiple_returns_first() {
        let output = " * [new branch]      first-branch -> first-branch\nBranch 'second-branch' set up to track remote.\n";
        assert_eq!(extract_branch(output), Some("first-branch".into()));
    }

    // -- strip_ansi tests --

    #[test]
    fn test_strip_ansi_empty() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn test_strip_ansi_no_codes() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn test_strip_ansi_basic() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
    }

    #[test]
    fn test_strip_ansi_multiple() {
        assert_eq!(strip_ansi("\x1b[1m\x1b[31mred bold\x1b[0m"), "red bold");
    }

    // -- find_url_with_path tests --

    #[test]
    fn test_find_url_basic() {
        let url = find_url_with_path(
            "Visit https://github.com/a/b/pull/1 now",
            "github.com",
            "/pull/",
        );
        assert_eq!(url, Some("https://github.com/a/b/pull/1".into()));
    }

    #[test]
    fn test_find_url_no_match() {
        let url = find_url_with_path("https://github.com/a/b/issues/1", "github.com", "/pull/");
        assert_eq!(url, None);
    }

    #[test]
    fn test_find_url_at_end_of_line() {
        let url = find_url_with_path("https://github.com/a/b/pull/99", "github.com", "/pull/");
        assert_eq!(url, Some("https://github.com/a/b/pull/99".into()));
    }

    #[test]
    fn test_find_url_with_trailing_delimiter() {
        let url = find_url_with_path(
            "PR: \"https://github.com/a/b/pull/5\" done",
            "github.com",
            "/pull/",
        );
        assert_eq!(url, Some("https://github.com/a/b/pull/5".into()));
    }

    // -- detect_rate_limit tests --

    #[test]
    fn test_detect_rate_limit_basic() {
        let output = "Processing...\nError: Rate limit exceeded. Please wait.\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited".into()));
    }

    #[test]
    fn test_detect_rate_limit_429() {
        let output = "HTTP 429: please retry later\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited (429)".into()));
    }

    #[test]
    fn test_detect_rate_limit_too_many_requests() {
        let output = "Error: too many requests, please slow down\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited: too many requests".into()));
    }

    #[test]
    fn test_detect_rate_limit_capacity() {
        let output = "The API is at capacity right now.\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited: at capacity".into()));
    }

    #[test]
    fn test_detect_rate_limit_overloaded() {
        let output = "Service is overloaded. Retrying in 30s.\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited: overloaded".into()));
    }

    #[test]
    fn test_detect_rate_limit_quota_exceeded() {
        let output = "Error: Quota exceeded for project.\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited: quota exceeded".into()));
    }

    #[test]
    fn test_detect_rate_limit_resource_exhausted() {
        let output = "RESOURCE_EXHAUSTED: API limit reached.\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited: resource exhausted".into()));
    }

    #[test]
    fn test_detect_rate_limit_no_match() {
        let output = "$ cargo test\nrunning 42 tests\nall passed\n";
        assert!(detect_rate_limit(output).is_none());
    }

    #[test]
    fn test_detect_rate_limit_with_ansi() {
        let output = "\x1b[31mRate limit exceeded\x1b[0m\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited".into()));
    }

    #[test]
    fn test_detect_rate_limit_case_insensitive() {
        let output = "RATE LIMIT warning: slow down\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited".into()));
    }

    #[test]
    fn test_detect_rate_limit_only_last_20_lines() {
        // Put the rate limit message beyond the last 20 lines
        let mut output = String::from("Rate limit exceeded\n");
        for _ in 0..25 {
            output.push_str("normal output line\n");
        }
        assert!(detect_rate_limit(&output).is_none());
    }

    #[test]
    fn test_detect_rate_limit_empty() {
        assert!(detect_rate_limit("").is_none());
    }
}
