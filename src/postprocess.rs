//! Post-processing of whatever the model returned.
//!
//! Models leak markdown fences and stray preamble even when told not to, so
//! we clean it up by default. `--raw` skips all of this.

/// Strip a wrapping ``` fence (with optional language tag) and surrounding
/// blank lines.
pub fn strip_fences(s: &str) -> String {
    let t = s.trim();
    if !t.starts_with("```") {
        return t.to_string();
    }
    let body = match t.find('\n') {
        Some(i) => &t[i + 1..],
        None => return t.to_string(),
    };
    let body = body.trim_end();
    let body = match body.strip_suffix("```") {
        Some(b) => b,
        None => return t.to_string(),
    };
    body.trim_matches('\n').to_string()
}

/// Remove common conversational preambles like "Here is the commit message:".
pub fn strip_preamble(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let mut start = 0;
    while start < lines.len() {
        let line = lines[start].trim();
        let lower = line.to_lowercase();
        let is_preamble = line.is_empty()
            || (line.ends_with(':')
                && (lower.contains("commit message")
                    || lower.contains("here is")
                    || lower.contains("here's")))
            || lower.starts_with("sure,")
            || lower.starts_with("certainly");
        if is_preamble {
            start += 1;
        } else {
            break;
        }
    }
    lines[start..].join("\n").trim_end().to_string()
}

/// Full cleanup pipeline for a single message.
pub fn clean(s: &str) -> String {
    strip_preamble(&strip_fences(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_diff_fence() {
        assert_eq!(strip_fences("```diff\nfeat: x\n```"), "feat: x");
        assert_eq!(
            strip_fences("```\nfeat: x\n\nbody\n```\n"),
            "feat: x\n\nbody"
        );
    }

    #[test]
    fn leaves_unfenced_alone() {
        assert_eq!(strip_fences("feat: x\n"), "feat: x");
    }

    #[test]
    fn strips_here_is_preamble() {
        assert_eq!(
            strip_preamble("Here is the commit message:\nfeat: x"),
            "feat: x"
        );
        assert_eq!(
            strip_preamble("Sure, here is a commit message:\n\nfix: y"),
            "fix: y"
        );
    }

    #[test]
    fn keeps_body_after_blank_line() {
        assert_eq!(strip_preamble("feat: x\n\n- does y"), "feat: x\n\n- does y");
    }

    #[test]
    fn full_cleanup() {
        assert_eq!(
            clean("```\nHere is the commit message:\nfix: z\n```"),
            "fix: z"
        );
    }
}
