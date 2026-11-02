# Architecture

`cmg` is a single-binary, single-shot CLI. One run does:

```
args ──▶ config ──▶ git ──▶ prompt ──▶ HTTP ──▶ postprocess ──▶ stdout
                                        │
                                        └──▶ (optional) git commit
```

There is no state between runs, no cache, no background process.

## Modules

| File | Responsibility |
| --- | --- |
| `src/main.rs` | Parses args, loads config, resolves the diff target, builds the prompt, calls the client, writes output, optionally commits. `cmg init` is dispatched here before clap. |
| `src/cli.rs` | `clap` derive struct. `TargetMode` (`Auto`/`Staged`/`Unstaged`/`All`) and the `git diff` arguments for each. |
| `src/config.rs` | Finds the config file, parses JSON or TOML into the same `Config` struct, applies defaults, validates the API key, writes the example config. |
| `src/git.rs` | Thin `Command::new("git")` wrapper. Diff, log, status, HEAD detection, empty-tree hash, and `git commit --file -`. |
| `src/prompt.rs` | System rules + user message (recent log, hint, diff). Diff size truncation. |
| `src/llm.rs` | `POST {base_url}/chat/completions` via `ureq`. Splits multi-message responses. |
| `src/postprocess.rs` | Strips markdown fences and conversational preambles from model output. |

## Data flow

### 1. Config resolution

First match wins:

1. `--config <PATH>`
2. `$CMG_CONFIG`
3. `$XDG_CONFIG_HOME/cmg/config.json`, then `config.toml`
4. `~/.config/cmg/config.json`, then `config.toml`

The format is chosen by file extension, falling back to "try JSON, then TOML".
Unknown fields are rejected (`deny_unknown_fields`), so a typo is an error
rather than a silently ignored setting. Sections other than `provider` are
optional and defaulted.

The API key comes from `provider.api_key_env` (preferred) or
`provider.api_key`. The environment variable wins when both are set. A missing
key is a load-time error, so we never make an unauthenticated request.

### 2. Target resolution

`Auto` (no flags) is resolved by diffing the index: if `git diff --cached` is
non-empty, that is the target; otherwise unstaged changes are used. In a
repository with no commits, the index is compared against git's empty tree.

If there are no tracked changes but untracked files exist, each is diffed
against `/dev/null` with `--no-index` (exit code 1 there just means "differs"),
giving a normal `new file` patch. Files that are empty produce no hunks, so a
plain name list is sent instead. `--path` filters apply to these files too.

Lockfiles and generated files (`Cargo.lock`, `*.min.js`, `dist/`, …) are
excluded via pathspecs (`:(exclude)…`) unless `diff.ignore_lockfiles` is false.
User `--path` filters and the exclusions share a single `--` pathspec list.

### 3. Prompt

The system prompt encodes the message rules: imperative mood, subject length,
Conventional Commits on/off, body only when warranted, output language, and
optional per-user `extra_instructions`. The user message carries up to ten
recent commit subjects (cheap style matching), an optional `--hint`, and the
diff inside a ```diff fence.

Diffs larger than `diff.max_chars` are cut on a line boundary (respecting UTF-8
boundaries) and the prompt records that it was truncated.

### 4. Request

A single non-streaming `chat/completions` call, `temperature` 0.3. HTTP status
errors are unwrapped to the provider's `error.message`. Connection errors and
malformed bodies get distinct messages so the common cause (wrong `base_url`,
not an OpenAI-compatible endpoint) is obvious.

### 5. Output

`--raw` prints the response verbatim. Otherwise the response is split on
`---` lines (for `--count`), each part has fences and preambles removed, and
duplicates are dropped. A single message may be multi-line (subject + body).
Multiple messages print subject lines only, one per line, so they pipe into
`head -1` or `fzf`.

`--commit` stages everything when the target was not already the index, then
pipes the message to `git commit --file -` so multi-line bodies and argv limits
are not a problem.

## Design decisions

- **Git subprocess over libgit2.** Zero native build dependencies, smaller
  binaries, and diffs that match what the user sees in `git diff`.
- **No async.** One request per run does not justify a runtime; `ureq` with
  rustls keeps the dependency tree and startup time small.
- **OpenAI-compatible only (v1).** One request/response shape covers OpenAI,
  OpenRouter, Groq, vLLM, LM Studio, and Ollama's `/v1`. Other provider shapes
  can be added later behind a config discriminator if there is demand.
- **Post-processing on by default.** Models leak fences and "Here is the commit
  message:" even when instructed not to; cleaning by default keeps stdout
  usable in `git commit -m "$(cmg)"`.
- **stdout is machine-readable.** All human-facing chatter is on stderr.

## Known limitations

- No streaming, so a slow model shows no progress (use `--verbose` for timings).
- No token counting; truncation is byte-based.
- `--commit` commits everything staged, so a partially selected index is
  respected only when you use the default staged target.
- Untracked files are only used when nothing tracked has changed; once there is
  a tracked diff they are ignored (stage them to include them).
