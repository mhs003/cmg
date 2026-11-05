//! Command line interface definition.
//!
//! Note on naming: `-c` belongs to `--commit` and `-s`/`-u` to the diff
//! selection flags. Everything else is long-only to keep the surface small.

use clap::Parser;

/// Generate a git commit message from your changes using an LLM.
///
/// By default the message is printed to stdout. Use `--commit` to also
/// create the commit. Without `--staged`/`--unstaged`/`--all` the target is
/// chosen automatically: staged changes win, otherwise unstaged ones are used.
#[derive(Debug, Clone, Parser)]
#[command(name = "cmg", version, about, long_about = None)]
pub struct Cli {
    /// Only consider staged changes (`git diff --cached`).
    #[arg(short = 's', long, conflicts_with_all = ["unstaged", "all"])]
    pub staged: bool,

    /// Only consider unstaged changes (`git diff`).
    #[arg(short = 'u', long, conflicts_with_all = ["staged", "all"])]
    pub unstaged: bool,

    /// Consider all changes since HEAD (staged + unstaged).
    #[arg(short = 'a', long, conflicts_with_all = ["staged", "unstaged"])]
    pub all: bool,

    /// Limit the diff to a path (repeatable).
    #[arg(long = "path", value_name = "PATH")]
    pub paths: Vec<String>,

    /// Write a Conventional Commits message (type(scope): subject).
    #[arg(long)]
    pub conventional: bool,

    /// Do not use Conventional Commits; write a plain sentence.
    #[arg(long, conflicts_with = "conventional")]
    pub plain: bool,

    /// Number of alternative messages to request, one per line.
    #[arg(short = 'n', long, value_name = "N", default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=5))]
    pub count: u8,

    /// Extra guidance for the model, e.g. "mention the typo in the parser".
    #[arg(short = 't', long = "type", value_name = "TEXT")]
    pub hint: Option<String>,

    /// Path to the config file (overrides $CMG_CONFIG and the default).
    #[arg(long = "config", value_name = "PATH")]
    pub config: Option<String>,

    /// Override the model from the config.
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Override the API base URL from the config.
    #[arg(long = "base-url", value_name = "URL")]
    pub base_url: Option<String>,

    /// Output the model response as-is (no fence stripping or validation).
    #[arg(short = 'r', long)]
    pub raw: bool,

    /// Print diagnostics to stderr.
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Create the commit with the generated message.
    #[arg(short = 'c', long)]
    pub commit: bool,

    /// Review the message, then commit, regenerate, or revise it with your
    /// feedback. Requires a terminal.
    //
    // The old `-w` / `--wait` / `--watch` spellings stay as hidden aliases so
    // existing scripts and aliases keep working; `deprecated_flags` warns when
    // they are used. A plain `//` comment, not `///`, keeps the note out of
    // `--help`.
    #[arg(
        short = 'i',
        long,
        aliases = ["wait", "watch"],
        short_aliases = ['w']
    )]
    pub interactive: bool,
}

/// The spelling users should write instead of the deprecated ones.
pub const PREFERRED_FLAG: &str = "-i (--interactive)";

/// Legacy spellings of `--interactive`, mapped to the flag they replaced them.
///
/// These are still accepted as hidden aliases. clap does not report which
/// alias matched, so the raw arguments are scanned instead (see
/// `deprecated_flags`).
const DEPRECATED_FLAGS: [&str; 3] = ["-w", "--wait", "--watch"];

/// Legacy flag spellings present in `args`, in the order they appear.
///
/// Only exact tokens count, and only up to a `--` separator: anything after it
/// is a positional argument, not a flag. Values belonging to other options are
/// skipped so `--type -w` or `--path=-w` do not trigger a false warning.
pub fn deprecated_flags(args: &[String]) -> Vec<&'static str> {
    let mut found = Vec::new();
    let mut takes_value = false;

    for arg in args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if takes_value {
            // The previous option was waiting for its value, whatever it looks
            // like. `--type=text` is handled below instead.
            takes_value = false;
            continue;
        }

        // `--watch` and `--watch=x` both name the option `watch`.
        let (name, inline_value) = match arg.strip_prefix("--") {
            Some(rest) => match rest.split_once('=') {
                Some((name, _)) => (name, true),
                None => (rest, false),
            },
            None => {
                if DEPRECATED_FLAGS.contains(&arg.as_str()) {
                    found.push(*DEPRECATED_FLAGS.iter().find(|f| *f == arg).unwrap());
                    continue;
                }
                // Short options that take a value, e.g. `-t --watch`.
                takes_value = SHORT_OPTS_WITH_VALUES.contains(&arg.as_str());
                continue;
            }
        };

        if let Some(flag) = DEPRECATED_FLAGS
            .iter()
            .find(|f| f.trim_start_matches('-') == name)
        {
            found.push(*flag);
        }
        takes_value = !inline_value && LONG_OPTS_WITH_VALUES.contains(&name);
    }
    found
}

/// Short options that take a value, so a following token is not a flag.
const SHORT_OPTS_WITH_VALUES: [&str; 2] = ["-t", "-n"];

/// Long options that take a value, so a following token is not a flag.
const LONG_OPTS_WITH_VALUES: [&str; 6] = ["path", "count", "type", "config", "model", "base-url"];

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        std::iter::once("cmg".to_string())
            .chain(s.split_whitespace().map(str::to_string))
            .collect()
    }

    #[test]
    fn each_legacy_spelling_is_detected() {
        assert_eq!(deprecated_flags(&args("-w")), vec!["-w"]);
        assert_eq!(deprecated_flags(&args("--wait")), vec!["--wait"]);
        assert_eq!(deprecated_flags(&args("--watch")), vec!["--watch"]);
    }

    #[test]
    fn current_spelling_is_not_deprecated() {
        assert!(deprecated_flags(&args("-i")).is_empty());
        assert!(deprecated_flags(&args("--interactive")).is_empty());
        assert!(deprecated_flags(&args("-w -i")).contains(&"-w"));
    }

    #[test]
    fn lookalikes_are_not_matched() {
        // Not exact tokens, so no warning.
        assert!(deprecated_flags(&args("--watchdog")).is_empty());
        assert!(deprecated_flags(&args("-wx")).is_empty());
    }

    #[test]
    fn values_that_look_like_flags_are_skipped() {
        assert!(deprecated_flags(&args("--type -w")).is_empty());
        assert!(deprecated_flags(&args("-t --watch")).is_empty());
        assert!(deprecated_flags(&args("--path=-w")).is_empty());
        assert!(deprecated_flags(&args("--type=--wait")).is_empty());
        assert!(deprecated_flags(&args("--count 2 -w")) == vec!["-w"]);
    }

    #[test]
    fn tokens_after_a_separator_are_not_flags() {
        assert!(deprecated_flags(&args("-- -w")).is_empty());
    }
}

impl Cli {
    /// Which diff to use. Explicit flags win; otherwise `Auto`, which is
    /// resolved later once we know which diffs are non-empty.
    pub fn target_mode(&self) -> TargetMode {
        if self.staged {
            TargetMode::Staged
        } else if self.unstaged {
            TargetMode::Unstaged
        } else if self.all {
            TargetMode::All
        } else {
            TargetMode::Auto
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetMode {
    /// Staged if any, else unstaged.
    Auto,
    Staged,
    Unstaged,
    All,
}

impl TargetMode {
    pub fn git_args(self, paths: &[String]) -> Vec<String> {
        let mut args: Vec<String> = ["diff", "--no-color", "--no-ext-diff"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        match self {
            TargetMode::Staged => args.push("--cached".into()),
            TargetMode::Unstaged => {}
            // Resolved before any diff is requested; HEAD makes `--all` also
            // work on a repository that has no commits yet.
            TargetMode::All => args.push("HEAD".into()),
            TargetMode::Auto => args.push("--cached".into()),
        }
        if !paths.is_empty() {
            args.push("--".into());
            args.extend(paths.iter().cloned());
        }
        args
    }
}
