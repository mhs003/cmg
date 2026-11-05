//! cmg — generate git commit messages with an LLM.
//!
//! stdout carries only the commit message so it pipes cleanly:
//!     git commit -m "$(cmg)"
//! Everything else (errors, diagnostics) goes to stderr.

mod cli;
mod config;
mod git;
mod llm;
mod postprocess;
mod prompt;
mod spinner;
mod wait;

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Parser;

use cli::{Cli, TargetMode};
use config::{Config, DiffConfig};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    // `cmg init`: scaffolding only, never talks to git or the API, so it is
    // dispatched before clap sees the arguments.
    if args.get(1).map(|s| s.as_str()) == Some("init") {
        return init_command(&args);
    }

    let cli = Cli::parse();
    // clap accepts the legacy spellings silently; tell the user which one they
    // typed and what replaced it. Resolved after parsing so a rejected command
    // line does not also print a deprecation note.
    for flag in cli::deprecated_flags(&args) {
        eprintln!(
            "cmg: warning: `{flag}` is deprecated and will be removed in a future \
             version; use {} instead",
            cli::PREFERRED_FLAG
        );
    }
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cmg: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn init_command(args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "Usage: cmg init [--toml] [--force]\n\n\
             Writes an example config to $XDG_CONFIG_HOME/cmg/config.json (or ~/.config/...).\n\
               --toml    write config.toml instead\n\
               --force   overwrite an existing config"
        );
        return ExitCode::SUCCESS;
    }
    let force = args.iter().any(|a| a == "--force" || a == "-f");
    let toml = args.iter().any(|a| a == "--toml");
    match config::write_example(force, toml) {
        Ok(p) => {
            eprintln!("cmg: wrote {}", p.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cmg: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let (mut cfg, cfg_path) = Config::load(cli.config.as_deref())?;
    if let Some(m) = &cli.model {
        cfg.provider.model = m.clone();
    }
    if let Some(u) = &cli.base_url {
        cfg.provider.base_url = u.clone();
    }

    if cli.interactive && !(io::stdin().is_terminal() && io::stderr().is_terminal()) {
        bail!("--interactive needs a terminal (stdin and stderr)");
    }

    if !git::is_repo() {
        bail!("not inside a git repository");
    }

    let target = resolve_target(cli)?;
    let diff = match git::diff(&diff_args(target, &cfg.diff, cli)) {
        Ok(d) => d,
        Err(e) if !git::has_head() && target != TargetMode::Unstaged => {
            // First commit in the repo: there is no HEAD to diff against, so
            // show the staged files as additions against the empty tree.
            if cli.verbose {
                eprintln!("cmg: {e:#}; diffing against the empty tree");
            }
            git::diff(&root_commit_args(&cfg.diff, cli))?
        }
        Err(e) => return Err(e),
    };
    if diff.trim().is_empty() {
        // Nothing tracked changed, but untracked files are still work the user
        // may want a message for: diff them against /dev/null.
        let untracked: Vec<String> = git::untracked()
            .into_iter()
            .filter(|f| path_matches(f, &cli.paths))
            .collect();
        if !untracked.is_empty() {
            if cli.verbose {
                eprintln!(
                    "cmg: no tracked changes; diffing {} untracked file(s)",
                    untracked.len()
                );
            }
            let untracked_diff = git::untracked_diff(&untracked);
            if !untracked_diff.trim().is_empty() {
                return generate(cli, &cfg, &cfg_path, target, untracked_diff);
            }
        }
        bail!("{}", empty_message(target, cli));
    }

    generate(cli, &cfg, &cfg_path, target, diff)
}

/// Everything after we have a diff: truncate, prompt, call, print or commit.
fn generate(
    cli: &Cli,
    cfg: &Config,
    cfg_path: &std::path::Path,
    target: TargetMode,
    diff: String,
) -> Result<()> {
    let (diff, truncated) = prompt::truncate(&diff, cfg.diff.max_chars);
    if truncated {
        eprintln!(
            "cmg: warning: diff truncated to {} bytes, increase diff.max_chars for more context",
            cfg.diff.max_chars
        );
    }

    let conventional = if cli.plain {
        false
    } else if cli.conventional {
        true
    } else {
        cfg.prompt.conventional
    };
    let log = if cfg.prompt.include_log_context {
        git::recent_log(10)
    } else {
        Vec::new()
    };

    let system = prompt::system(&cfg.prompt, conventional, cli.count);
    let input = prompt::PromptInput {
        cfg: &cfg.prompt,
        hint: cli.hint.as_deref(),
        diff: &diff,
        truncated,
    };
    let user = prompt::user(&input, &log);

    let client = llm::Client::new(
        &cfg.provider.base_url,
        cfg.resolve_api_key(),
        &cfg.provider.model,
        cfg.provider.timeout_secs,
        cfg.provider
            .extra_headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    );

    if cli.verbose {
        eprintln!(
            "cmg: config {} | model {} | url {} | target {:?} | prompt {} bytes",
            cfg_path.display(),
            client.model(),
            client.url(),
            target,
            user.len()
        );
    }

    let history = vec![
        llm::ChatMessage::new("system", &system),
        llm::ChatMessage::new("user", &user),
    ];

    if cli.interactive {
        return interactive_loop(cli, &client, target, history);
    }

    let messages = build_messages(&request(&client, &history, cli, "thinking")?, cli);
    let first = messages
        .first()
        .cloned()
        .context("model returned no usable message")?;

    if cli.commit {
        return do_commit(target, &first, messages.len());
    }

    write_messages(&messages)
}

/// One request, with the usual timing diagnostics.
///
/// Only interactive mode shows a spinner. Other runs stay silent because their
/// stderr is often a log, and a request usually finishes before a spinner
/// would tell the user anything.
fn request(
    client: &llm::Client,
    history: &[llm::ChatMessage],
    cli: &Cli,
    label: &str,
) -> Result<String> {
    let started = std::time::Instant::now();
    let spinner = cli.interactive.then(|| spinner::Spinner::start(label));
    let result = client.chat(history).context("generating commit message");
    // Stop before anything else writes to stderr, or the two would interleave.
    if let Some(spinner) = spinner {
        spinner.stop();
    }
    let raw = result?;
    if cli.verbose {
        eprintln!("cmg: response in {:.1}s", started.elapsed().as_secs_f64());
    }
    Ok(raw)
}

/// Review loop for `-i`: show the message, then commit, regenerate, revise
/// with feedback, or cancel. Prompts go to stderr; only a committed message
/// ever reaches stdout.
fn interactive_loop(
    cli: &Cli,
    client: &llm::Client,
    target: TargetMode,
    mut history: Vec<llm::ChatMessage>,
) -> Result<()> {
    // Track the length so "generate another" can drop the previous exchange
    // while "feedback" keeps it.
    let base = history.len();

    let response = request(client, &history, cli, "thinking")?;
    let mut current = first_message(&response, cli)?;

    loop {
        match wait::review(&current)? {
            wait::Outcome::Commit => {
                return do_commit(target, &current, 1);
            }
            wait::Outcome::Cancel => {
                // Still print the rejected message so it can be copied or
                // piped; the commit itself was explicitly declined.
                return write_messages(&[current]);
            }
            wait::Outcome::Regenerate => {
                history.truncate(base);
                let response = request(client, &history, cli, "thinking")?;
                current = first_message(&response, cli)?;
            }
            wait::Outcome::Feedback(text) => {
                // Keep every turn so repeated feedback keeps refining the same
                // message instead of starting over.
                history.push(llm::ChatMessage::new("assistant", &current));
                history.push(llm::ChatMessage::new("user", &prompt::revision(&text)));
                let response = request(client, &history, cli, "revising")?;
                current = first_message(&response, cli)?;
            }
        }
    }
}

fn first_message(raw: &str, cli: &Cli) -> Result<String> {
    build_messages(raw, cli)
        .into_iter()
        .next()
        .context("model returned no usable message")
}

/// Clean and de-duplicate the model output.
fn build_messages(raw: &str, cli: &Cli) -> Vec<String> {
    if cli.raw {
        return vec![raw.trim_end().to_string()];
    }
    let mut seen = Vec::new();
    for m in llm::split_alternatives(raw) {
        let cleaned = postprocess::clean(&m);
        if !cleaned.is_empty() && !seen.contains(&cleaned) {
            seen.push(cleaned);
        }
    }
    seen
}

fn write_messages(messages: &[String]) -> Result<()> {
    let mut stdout = io::stdout().lock();
    if messages.len() <= 1 {
        if let Some(m) = messages.first() {
            writeln!(stdout, "{m}")?;
        }
        return Ok(());
    }
    // Alternatives are printed one per line: subjects only, for easy piping
    // into `head -1`, `fzf`, etc.
    for m in messages {
        let subject = m.lines().next().unwrap_or("").trim();
        writeln!(stdout, "{subject}")?;
    }
    Ok(())
}

fn do_commit(target: TargetMode, message: &str, count: usize) -> Result<()> {
    if target != TargetMode::Staged {
        // `git commit --file -` would refuse to run with an empty index, which
        // is exactly the Auto/Unstaged case. Stage everything first.
        eprintln!("cmg: staging all changes before committing");
        stage_all()?;
    }
    if target == TargetMode::Unstaged {
        eprintln!("cmg: note: staged every change, not only the ones that produced this message");
    }
    if count > 1 {
        eprintln!("cmg: note: --commit uses the first of {count} alternatives");
    }
    git::commit(message)
}

/// Staged changes win when present; otherwise fall back to unstaged.
fn resolve_target(cli: &Cli) -> Result<TargetMode> {
    match cli.target_mode() {
        TargetMode::Auto => {
            // Never let the root-commit fallback hide an empty index: when
            // there is no HEAD the staged diff is checked against the empty
            // tree instead.
            let mut args = diff_args(TargetMode::Staged, &DiffConfig::default(), cli);
            if !git::has_head() {
                args = root_commit_args(&DiffConfig::default(), cli);
            }
            let staged = git::diff(&args)?;
            if staged.trim().is_empty() {
                Ok(TargetMode::Unstaged)
            } else {
                Ok(TargetMode::Staged)
            }
        }
        explicit => Ok(explicit),
    }
}

fn diff_args(mode: TargetMode, cfg: &DiffConfig, cli: &Cli) -> Vec<String> {
    let mut args = mode.git_args(&cli.paths);
    if cfg.context_lines != 3 {
        args.insert(1, format!("-U{}", cfg.context_lines));
    }
    if cfg.ignore_lockfiles {
        // git accepts a single `--` followed by both inclusions and exclude
        // pathspecs, so the user's paths and our exclusions share one list.
        if !args.iter().any(|a| a == "--") {
            args.push("--".to_string());
        }
        for p in config::IGNORED_PATHS {
            args.push((*p).to_string());
        }
    }
    args
}

/// Same as `diff_args` but against the empty tree, for the first commit.
fn root_commit_args(cfg: &DiffConfig, cli: &Cli) -> Vec<String> {
    let mut args = vec![
        "diff".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--cached".to_string(),
        git::empty_tree_hash(),
    ];
    if cfg.context_lines != 3 {
        args.insert(1, format!("-U{}", cfg.context_lines));
    }
    args.push("--".to_string());
    args.extend(cli.paths.iter().cloned());
    if cfg.ignore_lockfiles {
        for p in config::IGNORED_PATHS {
            args.push((*p).to_string());
        }
    }
    args
}

/// True for an untracked file when no `--path` filters were given, else when
/// it lives under (or is) one of them.
fn path_matches(file: &str, paths: &[String]) -> bool {
    paths.is_empty()
        || paths
            .iter()
            .any(|p| file == p || file.starts_with(&format!("{}/", p.trim_end_matches('/'))))
}

fn empty_message(target: TargetMode, cli: &Cli) -> String {
    let scope = if cli.paths.is_empty() {
        String::new()
    } else {
        format!(" in {}", cli.paths.join(", "))
    };
    match target {
        TargetMode::Staged => format!("no staged changes{scope} (stage them with `git add`)"),
        TargetMode::Unstaged => format!("no unstaged changes{scope}"),
        TargetMode::All => format!("no changes since HEAD{scope}"),
        TargetMode::Auto => format!("no staged or unstaged changes{scope}"),
    }
}

fn stage_all() -> Result<()> {
    let status = std::process::Command::new("git")
        .args(["add", "--all"])
        .status()
        .context("running git add")?;
    if !status.success() {
        bail!("git add --all failed");
    }
    Ok(())
}
