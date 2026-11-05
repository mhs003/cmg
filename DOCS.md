# cmg

`cmg` reads your changes, asks a chat model for a commit message, and prints
it. Everything is one binary with no runtime dependencies beyond `git`.

```sh
cmg > /dev/null && git commit -m "$(cmg)"
```

- [Synopsis](#synopsis)
- [Commands](#commands)
- [Options](#options)
- [How the diff is chosen](#how-the-diff-is-chosen)
- [Interactive mode](#interactive-mode)
- [Configuration](#configuration)
  - [Config file location](#config-file-location)
  - [provider](#provider)
  - [prompt](#prompt)
  - [diff](#diff)
  - [output](#output)
- [Environment variables](#environment-variables)
- [Exit codes](#exit-codes)
- [Examples](#examples)

## Synopsis

```
cmg [OPTIONS] [COMMAND]
```

Without a command, `cmg` generates a message and prints it to stdout.

## Commands

### `cmg init`

Writes an example config to the platform config directory.

```sh
cmg init            # config.json
cmg init --toml     # config.toml instead
cmg init -f         # overwrite an existing config
```

| Flag | Meaning |
| --- | --- |
| `--toml` | Write `config.toml` instead of `config.json`. |
| `-f`, `--force` | Overwrite an existing file. Without it, an existing config is left alone. |

If the *other* format already exists, `cmg init` refuses rather than writing a
second file: JSON takes precedence when both are present, so a new
`config.json` would silently shadow a working `config.toml`. Use `--force` if
that is really what you want.

### `cmg help`

Prints help. `cmg help init` prints help for `init`.

## Options

### Diff selection

| Option | Meaning |
| --- | --- |
| `-s`, `--staged` | Only staged changes (`git diff --cached`). |
| `-u`, `--unstaged` | Only unstaged changes (`git diff`). |
| `-a`, `--all` | Everything since `HEAD` (staged and unstaged). |
| `--path <PATH>` | Limit the diff to a path. Repeatable. |

These conflict with each other. Omitting all of them selects automatically —
see [How the diff is chosen](#how-the-diff-is-chosen).

### Message shape

| Option | Meaning |
| --- | --- |
| `--conventional` | Force a Conventional Commits subject: `type(scope): subject`. |
| `--plain` | Force a plain sentence, no type prefix. |
| `-n`, `--count <N>` | Ask for `N` alternatives (1–5). One per line, **subjects only**. |
| `-t`, `--type <TEXT>` | Extra guidance, e.g. `"mention the typo in the parser"`. |
| `-r`, `--raw` | Print the model output verbatim: no fence stripping, no validation. |

`--conventional` and `--plain` override `prompt.conventional` from the config.

With `-n`, only the subject line of each alternative is printed, so the output
pipes cleanly into `head -n1` or `fzf`.

### Provider overrides

| Option | Meaning |
| --- | --- |
| `--config <PATH>` | Config file to use, instead of `$CMG_CONFIG` or the default. |
| `--model <MODEL>` | Override `provider.model`. |
| `--base-url <URL>` | Override `provider.base_url`. |

### Doing things

| Option | Meaning |
| --- | --- |
| `-c`, `--commit` | Create the commit with the generated message. |
| `-i`, `--interactive` | Review before deciding. Needs a terminal. |
| `-v`, `--verbose` | Print diagnostics (model, URL, target, timings) to stderr. |
| `-V`, `--version` | Print the version and which config file is in use. |
| `-h`, `--help` | Print help. |

`--commit` stages **everything** with `git add --all` unless the changes were
already staged, because `git commit` refuses to run against an empty index.
It warns when it does this.

`-V` is the quickest way to find your config:

```
$ cmg -V
cmg 0.7.0
config: /home/you/.config/cmg/config.toml
```

## How the diff is chosen

Without `--staged`, `--unstaged`, or `--all`:

1. **If anything is staged, only staged changes are used.** Otherwise
   everything you staged would be swept into a commit whose message never saw
   those changes.
2. **Otherwise unstaged changes are used.**
3. **If nothing tracked has changed, untracked files are diffed** against
   `/dev/null`, so `cmg` still works before you `git add`. Empty untracked
   files fall back to a name-only list.

Lockfiles and build output are excluded from the diff when
`diff.ignore_lockfiles` is on (the default). See [diff](#diff).

## Interactive mode

`cmg -i` generates a message, then shows a menu:

```
  ● agent › suggesting commit message
  │
  │  feat(blog-page): add pagination
  │

   commit     generate another     feedback     cancel
```

| Choice | Key | Effect |
| --- | --- | --- |
| commit | `c` | Commit with this message. |
| generate another | `g` | Ask again from the same diff. |
| feedback | `f` | Type a note; the message is revised. |
| cancel | `q`, Esc, Ctrl-C | Exit 1. Nothing is written to stdout. |

Arrow keys and `j`/`k` move; Enter selects.

**Feedback** keeps the conversation, so repeated notes refine the same message
instead of starting over. **generate another** discards the earlier turns.

The feedback prompt supports the usual line editing:

| Keys | Effect |
| --- | --- |
| `←` `→` | Move one character |
| `Home` `End`, `Ctrl-A` `Ctrl-E` | Start / end of line |
| `Alt-←` `Alt-→` | Move by word |
| `Ctrl-W`, `Alt-Backspace` | Delete previous word |
| `Ctrl-K` `Ctrl-U` | Delete to end / start |
| `Delete` | Delete under the cursor |
| `Enter` | Submit |
| `Esc`, `Ctrl-C` | Back to the menu, message unchanged |

Interactive mode requires a terminal on both stdin and stderr, and refuses to
run otherwise. Prompts go to stderr; only the accepted message reaches stdout.

`-w`, `--wait`, and `--watch` are deprecated aliases for `-i`. They print a
warning and will be removed in a future version.

## Configuration

The config is JSON or TOML, detected by file extension. Every key is optional
except `provider.base_url`, `provider.model`, and an API key.

An unknown key is an error, not a silent no-op, so a typo like `max_char`
fails loudly.

### Config file location

First match wins:

1. `--config <PATH>`
2. `$CMG_CONFIG`
3. the platform config directory:
   - **Windows:** `%APPDATA%\cmg` — falls back to `%XDG_CONFIG_HOME%`, then
     `%HOME%` (Git Bash), then `%USERPROFILE%`
   - **elsewhere:** `$XDG_CONFIG_HOME/cmg`, then `~/.config/cmg`

Inside that directory, `config.json` is checked before `config.toml`.

`cmg -V` prints the file that was chosen.

### `provider`

Where the model lives. Works with anything that speaks
`POST /chat/completions`: OpenAI, OpenRouter, Groq, Together, vLLM, LM Studio,
Ollama's `/v1`.

| Key | Default | Meaning |
| --- | --- | --- |
| `base_url` | — | API root. `/chat/completions` is appended unless already present. **Required.** |
| `model` | — | Model name, passed through verbatim. **Required.** |
| `api_key` | `null` | Key written in the file. Prefer `api_key_env`. |
| `api_key_env` | `null` | Name of an environment variable holding the key. **Wins over `api_key`.** |
| `timeout_secs` | `60` | Request timeout. |
| `extra_headers` | `{}` | Extra headers on every request, for proxies. |

There is no API key in the example config: it sets `api_key_env` instead, so
the secret stays in your environment rather than on disk.

### `prompt`

How the message is written.

| Key | Default | Meaning |
| --- | --- | --- |
| `language` | `"en"` | Language for the message. `"en"` is left unmentioned to the model. |
| `conventional` | `true` | Use Conventional Commits. `--conventional` / `--plain` override it. |
| `max_subject_chars` | `72` | Subject length cap. |
| `include_body` | `true` | Allow a body. `false` means subject only. |
| `include_log_context` | `true` | Show the last 10 commit subjects as style context. |
| `extra_instructions` | `""` | Appended to the system prompt. Useful for house rules. |

### `diff`

What is sent to the model.

| Key | Default | Meaning |
| --- | --- | --- |
| `context_lines` | `3` | Context around each hunk, like `git diff -U<n>`. |
| `max_chars` | `60000` | Truncate the diff beyond this many bytes. A warning is printed. |
| `ignore_lockfiles` | `true` | Exclude lockfiles and build artefacts from the diff. |

With `ignore_lockfiles` on, these are excluded:

```
*.lock            Cargo.lock        package-lock.json   yarn.lock
pnpm-lock.yaml    go.sum            poetry.lock         uv.lock
*.min.js          *.min.css         *.map               *.snap
dist/*            build/*           vendor/*
```

### `output`

| Key | Default | Meaning |
| --- | --- | --- |
| `strip_code_fences` | `true` | Remove ```` ``` ```` fences and "Here is your commit message:"-style preambles. |

`--raw` bypasses this regardless of the setting.

### Full example

```json
{
  "provider": {
    "base_url": "https://api.openai.com/v1",
    "api_key_env": "OPENAI_API_KEY",
    "api_key": null,
    "model": "gpt-4o-mini",
    "timeout_secs": 60,
    "extra_headers": {}
  },
  "prompt": {
    "language": "en",
    "conventional": true,
    "max_subject_chars": 72,
    "include_body": true,
    "include_log_context": true,
    "extra_instructions": ""
  },
  "diff": {
    "context_lines": 3,
    "max_chars": 60000,
    "ignore_lockfiles": true
  },
  "output": {
    "strip_code_fences": true
  }
}
```

The same in TOML:

```toml
[provider]
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o-mini"
timeout_secs = 60

[prompt]
language = "en"
conventional = true
max_subject_chars = 72
include_body = true
include_log_context = true
extra_instructions = ""

[diff]
context_lines = 3
max_chars = 60000
ignore_lockfiles = true

[output]
strip_code_fences = true
```

## Environment variables

| Variable | Meaning |
| --- | --- |
| `CMG_CONFIG` | Config file path. Overridden by `--config`. |
| `XDG_CONFIG_HOME` | Config directory root on Unix. |
| `HOME` | Fallback config root on Unix; also honoured on Windows. |
| `APPDATA` | Config root on Windows (preferred there). |
| `USERPROFILE` | Last-resort config root on Windows. |
| `NO_COLOR` | Disable colour. Any value. |
| `TERM=dumb` | Also disables colour and the animated indicator. |

The API key variable is whatever `provider.api_key_env` names, normally
`OPENAI_API_KEY`.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success. The message is on stdout. |
| `1` | Anything else: no changes, missing config, missing key, HTTP or network
  error, or the user cancelled in interactive mode. |

stdout carries **only** the commit message. Progress, warnings, prompts, and
errors all go to stderr, so `$(cmg)` is safe to use. A cancellation writes
nothing to stdout, so a script can tell "declined" from "here is a message".

## Examples

```sh
# Staged changes if any, otherwise unstaged; print the message
cmg

# Review before committing
cmg -i

# Generate and commit in one step
cmg -c

# Three alternatives to pick from
cmg -n 3

# Only these paths, and mention the parser fix
cmg --path src/parser.rs --path tests/ -t "mention the parser fix"

# A different model for one run
cmg --model gpt-4o --base-url https://api.openai.com/v1

# Plain sentence instead of Conventional Commits
cmg --plain

# See what cmg is actually doing
cmg -v
```
