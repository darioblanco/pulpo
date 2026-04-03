/// Patterns that indicate the agent is waiting for user input.
/// Pre-lowercased for efficient matching against lowercased output lines.
const WAITING_PATTERNS: &[&str] = &[
    // Generic confirmation prompts
    "(y/n)",
    "[y/n]",
    "[yes/no]",
    "(yes/no)",
    "yes / no",
    "do you trust",
    "press enter",
    "approve this",
    "are you sure",
    "continue?",
    "confirm?",
    "proceed?",
    // Claude Code
    "(y)es",
    "(n)o",
    "(a)lways",
    "do you want to proceed",
    // Codex CLI
    "allow command?",
    // Gemini CLI
    "allow?",
    "approve?",
    // Aider
    "to the chat?",
    "apply edit?",
    "shell command?",
    "create new file",
    // Amazon Q
    "allow this action?",
    "accept suggestion?",
    // SSH/sudo
    "continue connecting (yes/no)",
    "'s password:",
    "[sudo] password",
];

/// Check if the terminal output suggests the agent is waiting for user input.
/// Inspects the last 5 lines of output for known prompt patterns and extra user-configured patterns.
pub fn detect_waiting_for_input(output: &str, extra_patterns: &[String]) -> bool {
    let last_lines: Vec<&str> = output.lines().rev().take(5).collect();
    for line in &last_lines {
        let lower = line.to_lowercase();
        for pattern in WAITING_PATTERNS {
            if lower.contains(pattern) {
                return true;
            }
        }
        for pattern in extra_patterns {
            if lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }
    }
    false
}

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

/// Patterns that indicate errors/failures in agent output.
/// Each entry is (pattern, human-readable label).
const ERROR_PATTERNS: &[(&str, &str)] = &[
    ("error[e", "Compile error"),
    ("panicked at", "Panic"),
    ("npm err!", "npm error"),
    ("typeerror:", "TypeError"),
    ("syntaxerror:", "SyntaxError"),
    ("referenceerror:", "ReferenceError"),
    ("build failed", "Build failed"),
    ("fatal:", "Fatal error"),
];

/// Detect compilation errors, test failures, and panics in agent output.
/// Checks the last 30 lines for known error patterns.
/// Returns a short label like "Compile error", "Test failed", etc.
pub fn detect_error(output: &str) -> Option<String> {
    let cleaned = strip_ansi(output);
    let last_lines: Vec<&str> = cleaned.lines().rev().take(30).collect();

    for line in &last_lines {
        let lower = line.to_lowercase();

        // Check for test failures with specific patterns (line-start for `error:`)
        let trimmed = lower.trim_start();
        if trimmed.starts_with("error:") {
            return Some("Compile error".to_owned());
        }

        // FAILED at line start or as standalone word for test failures
        if trimmed.starts_with("failed") || trimmed.contains("test failed") {
            return Some("Test failed".to_owned());
        }

        // FAIL with space (vitest/jest)
        if trimmed.starts_with("fail ") {
            return Some("Test failed".to_owned());
        }

        for &(pattern, label) in ERROR_PATTERNS {
            if lower.contains(pattern) {
                return Some(label.to_owned());
            }
        }
    }
    None
}

/// Extracted token and cost data from agent terminal output.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub session_cost_usd: Option<f64>,
}

/// Parse a number string that may have K/M suffix and commas.
/// Examples: `"7.49K"` → `7490`, `"4.5M"` → `4_500_000`, `"12,345"` → `12345`.
fn parse_number_with_suffix(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let last = s.as_bytes().last()?;
    let (num_part, multiplier) = match last {
        b'K' | b'k' => (&s[..s.len() - 1], 1_000.0),
        b'M' | b'm' => (&s[..s.len() - 1], 1_000_000.0),
        _ => (s, 1.0),
    };

    // Remove commas and parse as f64 (to handle decimals like 7.49K)
    let cleaned: String = num_part.chars().filter(|c| *c != ',').collect();
    let value: f64 = cleaned.parse().ok()?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some((value * multiplier).round() as u64)
}

/// Keywords that indicate a line contains cost/price information.
/// Dollar amounts are only extracted from lines containing these keywords
/// to avoid false positives from code output like `$100.00`.
const COST_KEYWORDS: &[&str] = &[
    "cost", "spent", "session", "message", "total", "price", "budget", "usage", "billing",
];

/// Check if a lowercased line contains any cost-related keyword.
fn has_cost_keyword(lower_line: &str) -> bool {
    COST_KEYWORDS.iter().any(|kw| lower_line.contains(kw))
}

/// Extract all dollar amounts from a line.
/// Matches `$X.XX`, `$X.XXX`, `$X`, etc.
fn extract_dollar_amounts(line: &str) -> Vec<f64> {
    let mut amounts = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            // Collect the number after $
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
                end += 1;
            }
            if end > start
                && let Ok(val) = line[start..end].parse::<f64>()
            {
                amounts.push(val);
            }
            i = end;
        } else {
            i += 1;
        }
    }
    amounts
}

/// Token category for keyword-proximity classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenCategory {
    Input,
    Output,
    Total,
    CacheWrite,
    CacheRead,
}

/// Keyword with its category and maximum allowed distance.
struct KeywordRule {
    category: TokenCategory,
    keyword: &'static str,
    max_distance: usize,
}

/// All keyword rules. The classifier picks the nearest matching keyword.
const KEYWORD_RULES: &[KeywordRule] = &[
    // Multi-word keywords (more specific)
    KeywordRule {
        category: TokenCategory::CacheWrite,
        keyword: "cache write",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::CacheWrite,
        keyword: "cache_write",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::CacheWrite,
        keyword: "cache creation",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::CacheRead,
        keyword: "cache hit",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::CacheRead,
        keyword: "cache_hit",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::CacheRead,
        keyword: "cache read",
        max_distance: 30,
    },
    // Standard keywords
    KeywordRule {
        category: TokenCategory::Input,
        keyword: "input",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::Input,
        keyword: "sent",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::Output,
        keyword: "output",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::Output,
        keyword: "received",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::Total,
        keyword: "total",
        max_distance: 30,
    },
    KeywordRule {
        category: TokenCategory::Total,
        keyword: "used",
        max_distance: 30,
    },
    // Short ambiguous keywords — tight window (line is padded with trailing space)
    KeywordRule {
        category: TokenCategory::Input,
        keyword: " in ",
        max_distance: 10,
    },
    KeywordRule {
        category: TokenCategory::Output,
        keyword: " out ",
        max_distance: 10,
    },
];

/// Find a number token starting at the given position in the line.
/// Returns `(raw_string, parsed_value, end_position)`.
fn find_number_at(line: &str, start: usize) -> Option<(usize, u64, usize)> {
    let bytes = line.as_bytes();
    let mut pos = start;

    // Must start with a digit
    if pos >= bytes.len() || !bytes[pos].is_ascii_digit() {
        return None;
    }

    // Collect digits, commas, dots, and K/M suffix
    let num_start = pos;
    while pos < bytes.len()
        && (bytes[pos].is_ascii_digit()
            || bytes[pos] == b','
            || bytes[pos] == b'.'
            || bytes[pos] == b'K'
            || bytes[pos] == b'k'
            || bytes[pos] == b'M'
            || bytes[pos] == b'm')
    {
        // K/M suffix ends the number
        if matches!(bytes[pos], b'K' | b'k' | b'M' | b'm') {
            pos += 1;
            break;
        }
        pos += 1;
    }

    let raw = &line[num_start..pos];
    let value = parse_number_with_suffix(raw)?;
    Some((num_start, value, pos))
}

/// Classify a number on a line by finding the nearest keyword within range.
/// Returns the category of the closest matching keyword.
fn classify_number(line_lower: &str, num_start: usize, num_end: usize) -> Option<TokenCategory> {
    let mut best: Option<(TokenCategory, usize)> = None; // (category, distance)

    for rule in KEYWORD_RULES {
        // Check window before the number
        let win_start = num_start.saturating_sub(rule.max_distance);
        let before = &line_lower[win_start..num_start];
        if let Some(pos) = before.rfind(rule.keyword) {
            // Distance = gap between keyword end and number start
            let keyword_end = win_start + pos + rule.keyword.len();
            let distance = num_start - keyword_end;
            if best.is_none() || distance < best.unwrap().1 {
                best = Some((rule.category, distance));
            }
        }

        // Check window after the number
        let win_end = (num_end + rule.max_distance).min(line_lower.len());
        let after = &line_lower[num_end..win_end];
        if let Some(pos) = after.find(rule.keyword) {
            // Distance = gap between number end and keyword start
            let distance = pos;
            if best.is_none() || distance < best.unwrap().1 {
                best = Some((rule.category, distance));
            }
        }
    }

    best.map(|(cat, _)| cat)
}

/// Extract token usage and cost from agent terminal output.
///
/// Uses keyword-proximity matching to classify numbers found near
/// recognizable keywords. Works across agents without per-agent code.
///
/// Scans all lines (output is already capped at 500 by `capture_output`).
/// Iterates from the bottom (most recent) up; first match per category wins.
pub fn extract_agent_usage(output: &str) -> Option<AgentUsage> {
    let cleaned = strip_ansi(output);
    let lines: Vec<&str> = cleaned.lines().collect();

    let mut usage = AgentUsage::default();
    let mut max_cost: Option<f64> = None;
    let mut found_anything = false;

    // Iterate from bottom (most recent output) to top
    for line in lines.iter().rev() {
        // Pad with trailing space so " out " matches at end of line
        let mut lower = line.to_lowercase();
        lower.push(' ');

        // Extract dollar amounts only from lines with cost-related keywords
        if has_cost_keyword(&lower) {
            for amount in extract_dollar_amounts(&lower) {
                if amount > 0.0 {
                    found_anything = true;
                    match max_cost {
                        Some(current) if amount > current => max_cost = Some(amount),
                        None => max_cost = Some(amount),
                        _ => {}
                    }
                }
            }
        }

        // Find and classify numbers on this line
        let mut pos = 0;
        while pos < lower.len() {
            if lower.as_bytes()[pos].is_ascii_digit() {
                if let Some((num_start, value, num_end)) = find_number_at(&lower, pos) {
                    if let Some(category) = classify_number(&lower, num_start, num_end) {
                        found_anything = true;
                        // First match per category wins (most recent line)
                        match category {
                            TokenCategory::Input if usage.input_tokens.is_none() => {
                                usage.input_tokens = Some(value);
                            }
                            TokenCategory::Output if usage.output_tokens.is_none() => {
                                usage.output_tokens = Some(value);
                            }
                            TokenCategory::Total if usage.total_tokens.is_none() => {
                                usage.total_tokens = Some(value);
                            }
                            TokenCategory::CacheWrite if usage.cache_write_tokens.is_none() => {
                                usage.cache_write_tokens = Some(value);
                            }
                            TokenCategory::CacheRead if usage.cache_read_tokens.is_none() => {
                                usage.cache_read_tokens = Some(value);
                            }
                            _ => {}
                        }
                    }
                    pos = num_end;
                } else {
                    pos += 1;
                }
            } else {
                pos += 1;
            }
        }
    }

    usage.session_cost_usd = max_cost;

    if found_anything { Some(usage) } else { None }
}

/// Parse `git diff --shortstat` output into (files, insertions, deletions).
/// Input looks like: ` 3 files changed, 42 insertions(+), 7 deletions(-)`
/// Any of the three parts may be missing.
pub fn parse_git_shortstat(output: &str) -> (Option<u32>, Option<u32>, Option<u32>) {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return (None, None, None);
    }

    let mut files: Option<u32> = None;
    let mut insertions: Option<u32> = None;
    let mut deletions: Option<u32> = None;

    for part in trimmed.split(',') {
        let part = part.trim();
        if part.contains("file") {
            files = extract_leading_number(part);
        } else if part.contains("insertion") {
            insertions = extract_leading_number(part);
        } else if part.contains("deletion") {
            deletions = extract_leading_number(part);
        }
    }

    (files, insertions, deletions)
}

/// Extract the first number from a string.
fn extract_leading_number(s: &str) -> Option<u32> {
    let num_str: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    if num_str.is_empty() {
        return None;
    }
    num_str.parse().ok()
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

    // -- PR URL edge cases --

    #[test]
    fn test_extract_pr_url_with_query_params() {
        let output = "https://github.com/owner/repo/pull/42?expand=1\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://github.com/owner/repo/pull/42?expand=1".into())
        );
    }

    #[test]
    fn test_extract_pr_url_with_fragment() {
        let output = "See https://github.com/owner/repo/pull/42#issuecomment-123\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://github.com/owner/repo/pull/42#issuecomment-123".into())
        );
    }

    #[test]
    fn test_extract_pr_url_surrounded_by_ansi_heavy() {
        // Multiple nested ANSI codes
        let output =
            "\x1b[1m\x1b[33m\x1b[4mhttps://github.com/owner/repo/pull/7\x1b[0m\x1b[0m\x1b[0m\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://github.com/owner/repo/pull/7".into())
        );
    }

    #[test]
    fn test_extract_pr_url_in_angle_brackets() {
        // Some terminals/tools wrap URLs in angle brackets
        let output = "PR created: <https://github.com/owner/repo/pull/10>\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://github.com/owner/repo/pull/10".into())
        );
    }

    #[test]
    fn test_extract_pr_url_in_parentheses() {
        let output = "See PR (https://github.com/owner/repo/pull/3)\n";
        assert_eq!(
            extract_pr_url(output),
            Some("https://github.com/owner/repo/pull/3".into())
        );
    }

    #[test]
    fn test_extract_pr_url_empty() {
        assert_eq!(extract_pr_url(""), None);
    }

    // -- Branch edge cases --

    #[test]
    fn test_extract_branch_with_deep_slashes() {
        let output = "remote: Create a pull request for 'feature/deep/nested/branch' on GitHub:\n";
        assert_eq!(
            extract_branch(output),
            Some("feature/deep/nested/branch".into())
        );
    }

    #[test]
    fn test_extract_branch_empty() {
        assert_eq!(extract_branch(""), None);
    }

    #[test]
    fn test_extract_branch_empty_quotes() {
        // Edge case: empty branch name in quotes
        let output = "remote: Create a pull request for '' on GitHub:\n";
        assert_eq!(extract_branch(output), None);
    }

    #[test]
    fn test_extract_branch_set_up_to_track_with_slashes() {
        let output =
            "Branch 'feature/auth/oauth2' set up to track remote branch 'feature/auth/oauth2'.\n";
        assert_eq!(extract_branch(output), Some("feature/auth/oauth2".into()));
    }

    // -- Rate limit edge cases --

    #[test]
    fn test_detect_rate_limit_429_in_url_is_false_positive() {
        // Known limitation: "429" in a URL will trigger a false positive
        let output = "See https://github.com/owner/repo/issues/429 for details\n";
        // This IS detected as rate limit — documenting the false positive
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited (429)".into()));
    }

    #[test]
    fn test_detect_rate_limit_mixed_case() {
        let output = "Too Many Requests - slow down\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited: too many requests".into()));
    }

    #[test]
    fn test_detect_rate_limit_overloaded_uppercase() {
        let output = "SERVICE OVERLOADED\n";
        let result = detect_rate_limit(output);
        assert_eq!(result, Some("Rate limited: overloaded".into()));
    }

    // -- strip_ansi edge cases --

    #[test]
    fn test_strip_ansi_non_csi_sequence() {
        // ESC followed by non-'[' character (like OSC: ESC])
        // The code skips ESC and the next char, then passes through the rest
        let input = "\x1b]some title\x07normal text";
        let result = strip_ansi(input);
        // ESC consumed, then next char ']' consumed (non-'[' path),
        // remaining "some title\x07normal text" passes through
        assert_eq!(result, "some title\x07normal text");
    }

    #[test]
    fn test_strip_ansi_esc_at_end() {
        // ESC at the very end of string
        let input = "hello\x1b";
        let result = strip_ansi(input);
        // ESC consumed, then chars.next() returns None, so nothing more consumed
        assert_eq!(result, "hello");
    }

    // -- find_url_with_path edge cases --

    #[test]
    fn test_find_url_multiple_urls_on_same_line() {
        let line = "Old: https://github.com/a/b/issues/1 New: https://github.com/a/b/pull/2";
        let url = find_url_with_path(line, "github.com", "/pull/");
        assert_eq!(url, Some("https://github.com/a/b/pull/2".into()));
    }

    #[test]
    fn test_find_url_in_square_brackets() {
        let line = "[https://github.com/a/b/pull/3]";
        let url = find_url_with_path(line, "github.com", "/pull/");
        assert_eq!(url, Some("https://github.com/a/b/pull/3".into()));
    }

    // -- detect_error tests --

    #[test]
    fn test_detect_error_rust_compiler() {
        let output = "Compiling my-crate v0.1.0\nerror[E0308]: mismatched types\n";
        assert_eq!(detect_error(output), Some("Compile error".into()));
    }

    #[test]
    fn test_detect_error_error_at_line_start() {
        let output = "running build\nerror: could not compile `foo`\n";
        assert_eq!(detect_error(output), Some("Compile error".into()));
    }

    #[test]
    fn test_detect_error_test_failed() {
        let output = "running 5 tests\nFAILED tests/integration.rs\n";
        assert_eq!(detect_error(output), Some("Test failed".into()));
    }

    #[test]
    fn test_detect_error_panic() {
        let output = "thread 'main' panicked at 'index out of bounds'\n";
        assert_eq!(detect_error(output), Some("Panic".into()));
    }

    #[test]
    fn test_detect_error_npm_err() {
        let output = "npm ERR! code ELIFECYCLE\nnpm ERR! errno 1\n";
        assert_eq!(detect_error(output), Some("npm error".into()));
    }

    #[test]
    fn test_detect_error_typeerror() {
        let output = "TypeError: Cannot read property 'foo' of undefined\n";
        assert_eq!(detect_error(output), Some("TypeError".into()));
    }

    #[test]
    fn test_detect_error_syntaxerror() {
        let output = "SyntaxError: Unexpected token }\n";
        assert_eq!(detect_error(output), Some("SyntaxError".into()));
    }

    #[test]
    fn test_detect_error_referenceerror() {
        let output = "ReferenceError: foo is not defined\n";
        assert_eq!(detect_error(output), Some("ReferenceError".into()));
    }

    #[test]
    fn test_detect_error_build_failed() {
        let output = "Build failed with 2 errors\n";
        assert_eq!(detect_error(output), Some("Build failed".into()));
    }

    #[test]
    fn test_detect_error_git_fatal() {
        let output = "fatal: not a git repository\n";
        assert_eq!(detect_error(output), Some("Fatal error".into()));
    }

    #[test]
    fn test_detect_error_vitest_fail() {
        let output = " FAIL  src/utils.test.ts > should work\n";
        assert_eq!(detect_error(output), Some("Test failed".into()));
    }

    #[test]
    fn test_detect_error_no_match() {
        let output = "$ cargo build\nCompiling my-crate v0.1.0\nFinished dev\n";
        assert!(detect_error(output).is_none());
    }

    #[test]
    fn test_detect_error_only_last_30_lines() {
        let mut output = String::from("error[E0308]: old error\n");
        for _ in 0..35 {
            output.push_str("normal output line\n");
        }
        assert!(detect_error(&output).is_none());
    }

    #[test]
    fn test_detect_error_with_ansi() {
        let output = "\x1b[31merror[E0308]: mismatched types\x1b[0m\n";
        assert_eq!(detect_error(output), Some("Compile error".into()));
    }

    #[test]
    fn test_detect_error_empty() {
        assert!(detect_error("").is_none());
    }

    #[test]
    fn test_detect_error_test_failed_keyword() {
        let output = "1 test failed out of 42\n";
        assert_eq!(detect_error(output), Some("Test failed".into()));
    }

    // -- parse_git_shortstat tests --

    #[test]
    fn test_parse_git_shortstat_full() {
        let output = " 3 files changed, 42 insertions(+), 7 deletions(-)";
        assert_eq!(parse_git_shortstat(output), (Some(3), Some(42), Some(7)));
    }

    #[test]
    fn test_parse_git_shortstat_insertions_only() {
        let output = " 1 file changed, 10 insertions(+)";
        assert_eq!(parse_git_shortstat(output), (Some(1), Some(10), None));
    }

    #[test]
    fn test_parse_git_shortstat_deletions_only() {
        let output = " 2 files changed, 5 deletions(-)";
        assert_eq!(parse_git_shortstat(output), (Some(2), None, Some(5)));
    }

    #[test]
    fn test_parse_git_shortstat_empty() {
        assert_eq!(parse_git_shortstat(""), (None, None, None));
    }

    #[test]
    fn test_parse_git_shortstat_whitespace() {
        assert_eq!(parse_git_shortstat("  \n  "), (None, None, None));
    }

    #[test]
    fn test_parse_git_shortstat_one_file() {
        let output = " 1 file changed, 1 insertion(+), 1 deletion(-)";
        assert_eq!(parse_git_shortstat(output), (Some(1), Some(1), Some(1)));
    }

    // -- extract_leading_number tests --

    #[test]
    fn test_extract_leading_number_basic() {
        assert_eq!(extract_leading_number("42 files changed"), Some(42));
    }

    #[test]
    fn test_extract_leading_number_empty() {
        assert_eq!(extract_leading_number("no numbers"), None);
    }

    #[test]
    fn test_extract_leading_number_with_prefix() {
        assert_eq!(extract_leading_number("abc 123 def"), Some(123));
    }

    // -- parse_number_with_suffix tests --

    #[test]
    fn test_parse_number_with_suffix_plain() {
        assert_eq!(parse_number_with_suffix("105"), Some(105));
    }

    #[test]
    fn test_parse_number_with_suffix_k() {
        assert_eq!(parse_number_with_suffix("7.49K"), Some(7490));
    }

    #[test]
    fn test_parse_number_with_suffix_k_lowercase() {
        assert_eq!(parse_number_with_suffix("1k"), Some(1000));
    }

    #[test]
    fn test_parse_number_with_suffix_m() {
        assert_eq!(parse_number_with_suffix("4.5M"), Some(4_500_000));
    }

    #[test]
    fn test_parse_number_with_suffix_m_lowercase() {
        assert_eq!(parse_number_with_suffix("1.2m"), Some(1_200_000));
    }

    #[test]
    fn test_parse_number_with_suffix_commas() {
        assert_eq!(parse_number_with_suffix("12,345"), Some(12345));
    }

    #[test]
    fn test_parse_number_with_suffix_zero() {
        assert_eq!(parse_number_with_suffix("0"), Some(0));
    }

    #[test]
    fn test_parse_number_with_suffix_empty() {
        assert_eq!(parse_number_with_suffix(""), None);
    }

    #[test]
    fn test_parse_number_with_suffix_letters() {
        assert_eq!(parse_number_with_suffix("abc"), None);
    }

    #[test]
    fn test_parse_number_with_suffix_whitespace() {
        assert_eq!(parse_number_with_suffix("  7.49K  "), Some(7490));
    }

    // -- extract_dollar_amounts tests --

    #[test]
    fn test_extract_dollar_amounts_single() {
        assert_eq!(extract_dollar_amounts("Total cost: $1.55"), vec![1.55]);
    }

    #[test]
    fn test_extract_dollar_amounts_multiple() {
        let amounts = extract_dollar_amounts("Cost: $0.03 message, $0.06 session.");
        assert_eq!(amounts, vec![0.03, 0.06]);
    }

    #[test]
    fn test_extract_dollar_amounts_none() {
        assert!(extract_dollar_amounts("no dollars here").is_empty());
    }

    #[test]
    fn test_extract_dollar_amounts_small() {
        assert_eq!(extract_dollar_amounts("Cost: $0.003"), vec![0.003]);
    }

    #[test]
    fn test_extract_dollar_amounts_whole() {
        assert_eq!(extract_dollar_amounts("Cost: $10"), vec![10.0]);
    }

    // -- extract_agent_usage integration tests --

    #[test]
    fn test_agent_usage_aider_basic() {
        let output = "Tokens: 10,675 sent, 101 received. Cost: $0.03 message, $0.06 session.\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.input_tokens, Some(10_675));
        assert_eq!(usage.output_tokens, Some(101));
        assert_eq!(usage.session_cost_usd, Some(0.06));
    }

    #[test]
    fn test_agent_usage_aider_with_cache() {
        let output = "Tokens: 10,675 sent, 1,024 cache write, 8,192 cache hit, 101 received. Cost: $0.03 message, $0.06 session.\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.input_tokens, Some(10_675));
        assert_eq!(usage.output_tokens, Some(101));
        assert_eq!(usage.cache_write_tokens, Some(1024));
        assert_eq!(usage.cache_read_tokens, Some(8192));
        assert_eq!(usage.session_cost_usd, Some(0.06));
    }

    #[test]
    fn test_agent_usage_claude_code_cost() {
        let output = "Total cost:            $0.55\nTotal duration (API):  6m 19.7s\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.session_cost_usd, Some(0.55));
    }

    #[test]
    fn test_agent_usage_codex() {
        let output = "Token usage: 7.49K total (7.38K input + 105 output)\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.input_tokens, Some(7380));
        assert_eq!(usage.output_tokens, Some(105));
        assert_eq!(usage.total_tokens, Some(7490));
    }

    #[test]
    fn test_agent_usage_opencode() {
        let output = "142 tok/s · 4.5M in · 19K out\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.input_tokens, Some(4_500_000));
        assert_eq!(usage.output_tokens, Some(19_000));
    }

    #[test]
    fn test_agent_usage_goose_context_bar() {
        // Goose uses a TUI context bar — no classifiable keywords near the numbers.
        // The format "(3548/32000 tokens)" has no input/output/total keywords, so
        // we don't extract from it. This is acceptable: Goose is a TUI widget that
        // may not even appear reliably in tmux capture-pane output.
        let output = "(3548/32000 tokens)\n";
        assert!(extract_agent_usage(output).is_none());
    }

    #[test]
    fn test_agent_usage_generic_input_output() {
        let output = "Input tokens: 1234\nOutput tokens: 5678\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.input_tokens, Some(1234));
        assert_eq!(usage.output_tokens, Some(5678));
    }

    #[test]
    fn test_agent_usage_generic_underscore() {
        let output = "input_tokens: 1000\noutput_tokens: 2000\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.input_tokens, Some(1000));
        assert_eq!(usage.output_tokens, Some(2000));
    }

    #[test]
    fn test_agent_usage_total_only() {
        let output = "Total tokens: 12345\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.total_tokens, Some(12345));
    }

    #[test]
    fn test_agent_usage_tokens_used() {
        let output = "tokens used: 5000\n";
        let usage = extract_agent_usage(output).unwrap();
        // "used" maps to Total category
        assert_eq!(usage.total_tokens, Some(5000));
    }

    #[test]
    fn test_agent_usage_empty() {
        assert!(extract_agent_usage("").is_none());
    }

    #[test]
    fn test_agent_usage_no_match() {
        assert!(extract_agent_usage("$ cargo build\nCompiling...\nDone.\n").is_none());
    }

    #[test]
    fn test_agent_usage_with_ansi() {
        let output = "\x1b[33mInput tokens: 100\x1b[0m\n\x1b[33mOutput tokens: 200\x1b[0m\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(200));
    }

    #[test]
    fn test_agent_usage_last_match_wins() {
        let output =
            "Input tokens: 100\nOutput tokens: 200\nInput tokens: 500\nOutput tokens: 600\n";
        let usage = extract_agent_usage(output).unwrap();
        // Last (bottom) match wins — 500/600
        assert_eq!(usage.input_tokens, Some(500));
        assert_eq!(usage.output_tokens, Some(600));
    }

    #[test]
    fn test_agent_usage_cost_takes_largest() {
        let output = "Cost: $0.03 message, $0.06 session.\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.session_cost_usd, Some(0.06));
    }

    #[test]
    fn test_agent_usage_with_commas() {
        let output = "Input tokens: 12,345\nOutput tokens: 67,890\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.input_tokens, Some(12345));
        assert_eq!(usage.output_tokens, Some(67890));
    }

    #[test]
    fn test_agent_usage_ignores_dollar_in_code_output() {
        // Dollar amounts in code without cost keywords should NOT be picked up
        let output = "let amount = $100.00;\nlet fee = $15.50;\n";
        assert!(extract_agent_usage(output).is_none());
    }

    #[test]
    fn test_agent_usage_cost_requires_keyword() {
        // Dollar amount WITH a cost keyword is accepted
        let output = "Total cost: $0.55\n";
        let usage = extract_agent_usage(output).unwrap();
        assert_eq!(usage.session_cost_usd, Some(0.55));
    }

    #[test]
    fn test_agent_usage_cost_zero_ignored() {
        // $0.00 is filtered out (amount > 0.0 check)
        let output = "Cost: $0.00 session.\n";
        assert!(extract_agent_usage(output).is_none());
    }
}
