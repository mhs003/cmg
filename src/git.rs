//! Thin wrapper around the `git` CLI.
//!
//! We shell out instead of linking libgit2: no native build dependencies, and
//! the diff honours your `git config` (attributes, diff drivers, pathspecs).

use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Run git with `args` and return stdout as a String.
fn git(args: &[&str]) -> Result<String> {
    let out = run(args)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("git {} failed: {}", args.join(" "), err.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run(args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("running git (is it installed?)")
}

/// Diff an untracked file against `/dev/null`, giving a normal `new file`
/// patch. `--no-index` exits 1 when the files differ, which is the normal
/// case here, so a non-empty stdout is what matters.
fn diff_untracked_file(path: &str) -> String {
    let out = match run(&[
        "diff",
        "--no-index",
        "--no-color",
        "--no-ext-diff",
        "--",
        "/dev/null",
        path,
    ]) {
        Ok(o) => o,
        Err(_) => return String::new(),
    };
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Combined diff for untracked files. Content patches where possible (empty
/// files produce no hunks), plus a path list when nothing textual was
/// produced, so the model still knows what was added.
pub fn untracked_diff(files: &[String]) -> String {
    let mut out = String::new();
    for f in files {
        let d = diff_untracked_file(f);
        // Rewrite the `1/` and `2/` prefixes `--no-index` adds.
        let d = d
            .replace("diff --git 1/", "diff --git a/")
            .replace(" 2/", " b/");
        out.push_str(&d);
    }
    if out.is_empty() && !files.is_empty() {
        out.push_str("New files (contents empty):\n");
        for f in files {
            out.push_str("  ");
            out.push_str(f);
            out.push('\n');
        }
    }
    out
}

/// True when the process is inside a work tree.
pub fn is_repo() -> bool {
    git(&["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// True when the repository has at least one commit.
pub fn has_head() -> bool {
    git(&["rev-parse", "--verify", "--quiet", "HEAD"])
        .map(|_| true)
        .unwrap_or(false)
}

/// Hash of git's canonical empty tree, used to diff the very first commit.
pub fn empty_tree_hash() -> String {
    git(&["hash-object", "-t", "tree", "/dev/null"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string())
}

/// Return the diff for the given `git diff` arguments.
pub fn diff(args: &[String]) -> Result<String> {
    let mut argv: Vec<&str> = Vec::with_capacity(args.len());
    for a in args {
        argv.push(a);
    }
    git(&argv)
}

/// Untracked files that are not ignored.
pub fn untracked() -> Vec<String> {
    git(&["ls-files", "--others", "--exclude-standard"])
        .map(|s| s.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

/// Recent commit subjects, used as style reference for the model.
pub fn recent_log(n: usize) -> Vec<String> {
    git(&[
        "log",
        "--no-merges",
        "--pretty=format:%s",
        &format!("-{}", n),
    ])
    .map(|s| s.lines().map(|l| l.to_string()).collect())
    .unwrap_or_default()
}

/// Create a commit with `message` (supports multi-line bodies). The message
/// is passed on stdin via `-F -` so it never hits the shell or argv limits.
pub fn commit(message: &str) -> Result<()> {
    use std::io::Write;
    use std::process::Child;

    let mut child: Child = Command::new("git")
        .args(["commit", "--file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("running git commit")?;
    {
        let stdin = child.stdin.as_mut().context("opening git commit stdin")?;
        stdin.write_all(message.as_bytes())?;
        if !message.ends_with('\n') {
            stdin.write_all(b"\n")?;
        }
    }
    let status = child.wait()?;
    if !status.success() {
        bail!("git commit failed");
    }
    Ok(())
}
