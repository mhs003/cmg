# cmg

Commit messages from your diff, via any OpenAI-compatible API.

```
$ cmg
feat(wait): give the review loop a readable transcript
```

Prints to stdout, so it pipes:

```sh
git commit -m "$(cmg)"
```

## Install

Binaries are on the [releases page](https://github.com/mhs003/cmg/releases) —
Linux (glibc or static musl), Termux, macOS, Windows.

```sh
cargo install --path .          # or
cargo build --release           # target/release/cmg
```

## Configure

```sh
cmg init              # writes config.json to the config dir
cmg init --toml       # write config.toml instead
cmg -V                # version and which config file is in use
```

The config dir is `%APPDATA%\cmg` on Windows, otherwise
`$XDG_CONFIG_HOME/cmg` or `~/.config/cmg`. `cmg -V` prints the resolved path,
which saves guessing. If both `config.json` and `config.toml` exist, JSON wins.

Then set your key. The file defaults to reading `OPENAI_API_KEY`:

```json
{
  "provider": {
    "base_url": "https://api.openai.com/v1",
    "api_key_env": "OPENAI_API_KEY",
    "model": "gpt-4o-mini"
  }
}
```

Config is looked up in `--config`, `$CMG_CONFIG`, then the platform config dir.
TOML works too. Rest of the file:

```json
{
  "prompt": {
    "language": "en",
    "conventional": true,
    "max_subject_chars": 72,
    "include_body": true,
    "include_log_context": true,
    "extra_instructions": ""
  },
  "diff": { "context_lines": 3, "max_chars": 60000, "ignore_lockfiles": true },
  "output": { "strip_code_fences": true }
}
```

`base_url` is anything that speaks `POST /chat/completions`: OpenAI,
OpenRouter, Groq, vLLM, LM Studio, Ollama's `/v1`.

## Use

```sh
cmg                  # staged changes if any, otherwise unstaged
cmg -i               # review it first
cmg -c               # generate and commit
cmg -n 3             # three alternatives, one per line
cmg -t "mention the parser" -s
```

`-i` generates a message, then waits on a menu: **commit**, **generate
another**, **feedback**, **cancel**. Feedback sends your note back to the model
with the previous message in context, so repeated notes keep refining the same
message. Prompts go to stderr; only the message reaches stdout.

Untracked files count when nothing tracked has changed, so `cmg` also works
before you `git add`.

## Notes

- Lockfiles and generated files are excluded from the diff.
- Model output has fences and preambles stripped unless `--raw`.
- No TUI. The menu is one redrawn line; `NO_COLOR` is honoured, and turns the
  `thinking…` indicator into a single static line.
- The indicator goes to stderr, so it never pollutes piped stdout.
- Exit code is 1 on any failure, and messages go to stderr.

## Licence

MIT, see [LICENSE](LICENSE). Releases are in [CHANGELOG.md](CHANGELOG.md).
