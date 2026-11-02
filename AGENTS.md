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

## Style

- Rust 2021, `rustfmt` defaults, `cargo clippy --all-targets` clean.
- `anyhow::Result` for fallible functions; bail with lowercase, actionable
  messages that say what to do next (`... stage them with `git add <file>``).
- Small functions, no premature abstraction, no traits until there is a second
  implementation.
- Comments explain *why*, not *what*. Keep them short.

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
