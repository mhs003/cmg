# AGENTS.md

Guidance for AI agents (and humans) working in this repository.

## What this project is

`cmg` is a small Rust CLI that asks an OpenAI-compatible chat endpoint for a
git commit message and prints it to stdout. No TUI, no interactive prompts, no
daemon. If a change makes the tool heavier or chattier, it is probably wrong.

## Hard rules

1. **stdout is a contract.** Only the commit message (or one message per line
   with `--count`) may go to stdout. Progress, warnings, notes, and errors go
   to stderr. Never print colour codes or status noise to stdout.
2. **Exit codes matter.** `0` on success, `1` on any failure. An empty diff,
   missing config, missing API key, or HTTP error must exit non-zero with a
   message on stderr.
3. **No async runtime.** No tokio, no futures. The tool does exactly one
   blocking HTTP request per run.
4. **Shell out to `git`.** Do not add `git2`/libgit2. The project deliberately
   relies on the `git` binary so user `git config` (attributes, diff drivers,
   pathspecs) is honoured and there are no native build dependencies.
5. **Config is data, not code.** New settings belong in `src/config.rs` with a
   `#[serde(default)]` so existing config files keep working. Keep the JSON and
   TOML examples (`EXAMPLE_JSON` / `EXAMPLE_TOML`) in sync with the structs.
6. **Secrets never go to stderr.** Do not echo API keys or full request bodies
   in error paths.

## Layout

```
src/main.rs         entry point, orchestration, target selection, commit
src/cli.rs          clap definitions + how each target maps to git args
src/config.rs       config discovery, parsing (JSON/TOML), validation, defaults
src/git.rs          git subprocess helpers
src/prompt.rs       prompt construction and diff truncation
src/llm.rs          OpenAI-compatible HTTP client
src/postprocess.rs  fence/preamble stripping of model output
src/watch.rs        `-w` review loop: arrow-key menu, feedback input
tests/cli.rs        end-to-end tests against a local stub HTTP server
```

See `ARCHITECTURE.md` for how these fit together.

## Target selection (do not regress this)

Without an explicit flag the rule is: **if anything is staged, diff the staged
changes; otherwise diff unstaged changes.** This is implemented in
`resolve_target`. Explicit `--staged`, `--unstaged`, and `--all` always win.

Untracked files are not part of `git diff`. When there are no tracked changes
but untracked files exist, diff them against `/dev/null` (`git::untracked_diff`)
so they still produce a message; empty files fall back to a name-only list.
`--commit` stages everything first when the target was not already staged.

## Watch mode (`-w`)

An interactive review loop, and the only part that is allowed to hold the
terminal. Rules that keep it from becoming a TUI:

- Requires a TTY on both stdin and stderr; refuse with an error otherwise.
- Raw mode is enabled only for the menu and always restored, even on error.
- Prompts, the rendered message, and the menu go to **stderr**. stdout still
  receives only the message (the accepted one, or the rejected one on cancel).
- `feedback` keeps the conversation history (`llm::ChatMessage`), so repeated
  feedback refines the same message; `generate another` truncates back to the
  original diff turn.

## Style

- Rust 2021, `rustfmt` defaults, `cargo clippy --all-targets` clean.
- `anyhow::Result` for fallible functions; bail with lowercase, actionable
  messages that say what to do next (`... stage them with `git add <file>``).
- Small functions, no premature abstraction, no traits until there is a second
  implementation.
- Comments explain *why*, not *what*. Keep them short.

## Releases

Releases are automatic. **Every push to `master`** runs
`.github/workflows/release.yml`, which:

1. Reads the current version from `VERSION` (the single source of truth).
2. Scans Conventional Commits since the last `v*` tag and bumps the version:
   `feat`/breaking → minor, otherwise patch. A push containing only
   `docs`/`chore`/`ci`/`test`/`style` commits releases nothing.
3. Prepends a section to `CHANGELOG.md`, updates `VERSION` and the version in
   `Cargo.toml`, then commits `chore(release): vX.Y.Z`, tags it, and pushes.
4. Builds the binary matrix and publishes a GitHub release whose body is that
   changelog section.

Notes:

- `VERSION` and `Cargo.toml` must agree; `tests/meta.rs` enforces it.
- Never hand-edit a released changelog section; add entries by committing, not
  by editing history.
- Commit subjects are user-facing: they become changelog entries. Write them
  accordingly (`fix(menu): stop drift`, not `fix stuff`).
- `chore(release):` commits are skipped, which stops the bot's own push from
  triggering another release.
- **Termux is Android.** Use the `*-linux-android` targets with the NDK
  linker; glibc or musl Linux binaries do not run under Termux.
- `ring` (via rustls) contains C and assembly. Setting cargo's `linker` is
  **not enough**: `cc-rs` also needs a C compiler and picks its name from the
  target triple. For `armv7-linux-androideabi` that name
  (`arm-linux-androideabi-clang`) does not exist in the NDK, which ships
  `armv7a-...`. Export `CC_<target_with_underscores>` and `AR_<...>` as well.
- Packaging must be portable: `sha256sum` is GNU-only (macOS has `shasum`,
  Windows has `certutil`), and `7z` is not on every runner (`zip` is).
- `set -euo pipefail` plus a missing tool is the usual cause of a
  "Process completed with exit code 1" on an otherwise fine build.
- New targets go in the matrix with the right `bin`/`zip`/linker knobs; do not
  hard-code runner paths such as a specific NDK version.

Helper: `scripts/release-notes.sh <current> <range>` does step 2–3 locally
(exit 3 = nothing to release). Point it at scratch files with `VERSION_FILE` /
`CHANGELOG_FILE` to try it safely.

## Testing

```sh
cargo test                      # unit tests, fast, no network
cargo test --test cli -- --ignored   # end-to-end tests (needs git + built binary)
```

The end-to-end tests spin up a one-shot TCP stub and never hit the network.
Add a test for every behaviour change; bug fixes get a regression test.

## Commits

Use Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`,
`chore:`), imperative mood, subject under 72 characters. Commit at meaningful
states where `cargo test` passes — not half-finished edits.
