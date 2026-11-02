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

    /// Wait mode: review the message, then commit, regenerate, or revise it
    /// with your feedback. Requires a terminal.
    #[arg(short = 'w', long)]
    pub wait: bool,
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
