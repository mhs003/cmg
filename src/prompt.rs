//! Prompt construction.

use crate::config::Prompt as PromptConfig;

/// Everything the prompt needs that is not derived from the diff itself.
pub struct PromptInput<'a> {
    pub cfg: &'a PromptConfig,
    pub hint: Option<&'a str>,
    pub diff: &'a str,
    /// True when the diff was cut down to `diff.max_chars`.
    pub truncated: bool,
}

pub fn system(cfg: &PromptConfig, conventional: bool, count: u8) -> String {
    let mut s = String::new();
    s.push_str("You write git commit messages. You are given a unified diff.\n\n");

    s.push_str("Rules:\n");
    s.push_str("- Output ONLY the commit message. No preamble, no explanation, no markdown, no code fences.\n");
    s.push_str("- Describe the change as if completing the sentence \"This commit will ...\".\n");
    s.push_str(
        "- Use the imperative mood in the subject line (\"add\", \"fix\", not \"added\").\n",
    );
    s.push_str("- Do not end the subject with a period.\n");
    s.push_str(&format!(
        "- Keep the subject line at most {} characters.\n",
        cfg.max_subject_chars
    ));
    if conventional {
        s.push_str(
            "- Format: `type(optional-scope): subject` using one of feat, fix, refactor, docs, test, chore, perf, build, ci, style.\n",
        );
    } else {
        s.push_str("- Format: a single plain sentence, no type prefix.\n");
    }
    if cfg.include_body {
        s.push_str("- Add a body only when the change is non-trivial: one blank line, then a short bulleted explanation of the *why*, wrapped at 72 columns. Keep it under 5 bullets. Omit the body for small or obvious changes.\n");
    } else {
        s.push_str("- Output the subject line only. Never add a body.\n");
    }
    if !cfg.language.is_empty() && cfg.language != "en" {
        s.push_str(&format!(
            "- Write the message in language: {}.\n",
            cfg.language
        ));
    }
    if !cfg.extra_instructions.trim().is_empty() {
        s.push_str(&format!(
            "- Additional instructions: {}\n",
            cfg.extra_instructions.trim()
        ));
    }
    if count > 1 {
        s.push_str(&format!(
            "\nProduce exactly {} DIFFERENT alternatives. Separate them with a line containing only `---`. Each alternative is a complete commit message.\n",
            count
        ));
    }
    s
}

pub fn user(input: &PromptInput<'_>, log: &[String]) -> String {
    let mut out = String::new();

    if input.cfg.include_log_context && !log.is_empty() {
        out.push_str(
            "Recent commit subjects in this repository (match their tone and terminology):\n",
        );
        for line in log.iter().take(10) {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }

    if let Some(hint) = input.hint {
        if !hint.trim().is_empty() {
            out.push_str("Author's note about this change: ");
            out.push_str(hint.trim());
            out.push('\n');
        }
    }

    if input.truncated {
        out.push_str(
            "NOTE: the diff below was truncated to fit a size limit. Base the message on what is visible.\n",
        );
    }

    out.push_str("\nDiff:\n```diff\n");
    out.push_str(input.diff);
    if !input.diff.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n");
    out
}

/// Follow-up turn when the user asks for a revision in wait mode. The
/// previous message stays in the history so repeated feedback keeps refining
/// the same message rather than starting over.
pub fn revision(feedback: &str) -> String {
    format!(
        "Revise your previous commit message according to this feedback and output only the \
         revised message, following the same rules as before.\n\nFeedback: {}\n",
        feedback.trim()
    )
}

/// Cut a diff to `max` bytes on a line boundary, returning `(text, truncated)`.
pub fn truncate(diff: &str, max: usize) -> (String, bool) {
    if diff.len() <= max {
        return (diff.to_string(), false);
    }
    let mut cut = max;
    while cut > 0 && !diff.is_char_boundary(cut) {
        cut -= 1;
    }
    match diff[..cut].rfind('\n') {
        Some(i) if i > cut / 2 => (diff[..i].to_string(), true),
        _ => (diff[..cut].to_string(), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Prompt as PromptConfig;

    fn cfg() -> PromptConfig {
        PromptConfig::default()
    }

    #[test]
    fn revision_embeds_feedback() {
        let r = revision("  use fix type  ");
        assert!(r.contains("Feedback: use fix type"));
        assert!(r.contains("previous commit message"));
    }

    #[test]
    fn truncate_short_is_noop() {
        let (s, t) = truncate("abc\n", 100);
        assert_eq!(s, "abc\n");
        assert!(!t);
    }

    #[test]
    fn truncate_cuts_on_line_boundary() {
        let d = "line one\nline two\nline three\n";
        let (s, t) = truncate(d, 20);
        assert!(t);
        assert_eq!(s, "line one\nline two");
    }

    #[test]
    fn truncate_respects_utf8_boundaries() {
        let d = "héllo wörld ".repeat(20);
        let (s, t) = truncate(&d, 25);
        assert!(t);
        assert!(d.starts_with(&s));
    }

    #[test]
    fn system_mentions_conventional_when_enabled() {
        let s = system(&cfg(), true, 1);
        assert!(s.contains("type(optional-scope)"));
        assert!(s.contains("72 characters"));
    }

    #[test]
    fn system_plain_has_no_type_prefix() {
        let s = system(&cfg(), false, 1);
        assert!(s.contains("plain sentence"));
    }

    #[test]
    fn system_requests_alternatives() {
        let s = system(&cfg(), true, 3);
        assert!(s.contains("exactly 3 DIFFERENT"));
    }

    #[test]
    fn user_includes_log_and_hint() {
        let log = vec!["feat: x".to_string()];
        let input = PromptInput {
            cfg: &cfg(),
            hint: Some("typo fix"),
            diff: "+a\n",
            truncated: false,
        };
        let u = user(&input, &log);
        assert!(u.contains("feat: x"));
        assert!(u.contains("typo fix"));
        assert!(u.contains("```diff"));
    }
}
