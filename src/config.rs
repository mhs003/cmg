//! Configuration loading.
//!
//! Lookup order (first existing file wins):
//!   1. `--config <PATH>`
//!   2. `$CMG_CONFIG`
//!   3. the platform config dir (see `config_dir_from`), holding
//!      `config.json` or `config.toml`
//!
//! Both JSON and TOML are accepted; the format is detected from the file
//! extension and falls back to "try JSON, then TOML".

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const EXAMPLE_JSON: &str = r#"{
  "provider": {
    "base_url": "https://api.openai.com/v1",
    "api_key_env": "OPENAI_API_KEY",
    "api_key": null,
    "model": "gpt-4o-mini",
    "timeout_secs": 60
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
"#;

pub const EXAMPLE_TOML: &str = r#"[provider]
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
"#;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub provider: Provider,
    #[serde(default)]
    pub prompt: Prompt,
    #[serde(default)]
    pub diff: DiffConfig,
    #[serde(default)]
    pub output: Output,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    /// Base URL, e.g. `https://api.openai.com/v1`. `/chat/completions` is
    /// appended to it.
    pub base_url: String,
    /// Model name passed through verbatim.
    pub model: String,
    /// API key inline. Prefer `api_key_env` to keep secrets off disk.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Name of an environment variable holding the API key. Wins over
    /// `api_key` when set.
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Extra headers sent with every request (useful for proxies).
    #[serde(default)]
    pub extra_headers: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_true")]
    pub conventional: bool,
    #[serde(default = "default_subject_chars")]
    pub max_subject_chars: usize,
    #[serde(default = "default_true")]
    pub include_body: bool,
    #[serde(default = "default_true")]
    pub include_log_context: bool,
    #[serde(default)]
    pub extra_instructions: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiffConfig {
    #[serde(default = "default_context")]
    pub context_lines: u32,
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
    #[serde(default = "default_true")]
    pub ignore_lockfiles: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    #[serde(default = "default_true")]
    pub strip_code_fences: bool,
}

fn default_timeout() -> u64 {
    60
}
fn default_language() -> String {
    "en".into()
}
fn default_subject_chars() -> usize {
    72
}
fn default_context() -> u32 {
    3
}
fn default_max_chars() -> usize {
    60_000
}
fn default_true() -> bool {
    true
}

impl Default for Prompt {
    fn default() -> Self {
        Self {
            language: default_language(),
            conventional: true,
            max_subject_chars: default_subject_chars(),
            include_body: true,
            include_log_context: true,
            extra_instructions: String::new(),
        }
    }
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            context_lines: default_context(),
            max_chars: default_max_chars(),
            ignore_lockfiles: true,
        }
    }
}

impl Default for Output {
    fn default() -> Self {
        Self {
            strip_code_fences: true,
        }
    }
}

/// Paths passed to `git diff` to keep noise out. Lockfiles and generated
/// bundles burn tokens without telling the model anything useful.
pub const IGNORED_PATHS: &[&str] = &[
    ":(exclude)*.lock",
    ":(exclude)Cargo.lock",
    ":(exclude)package-lock.json",
    ":(exclude)yarn.lock",
    ":(exclude)pnpm-lock.yaml",
    ":(exclude)go.sum",
    ":(exclude)poetry.lock",
    ":(exclude)uv.lock",
    ":(exclude)*.min.js",
    ":(exclude)*.min.css",
    ":(exclude)*.map",
    ":(exclude)dist/*",
    ":(exclude)build/*",
    ":(exclude)vendor/*",
    ":(exclude)*.snap",
];

impl Config {
    pub fn load(explicit: Option<&str>) -> Result<(Self, PathBuf)> {
        let path = match resolve_path(explicit)? {
            Some(p) => p,
            None => bail!(
                "no config file found.\n\
                 Create one with `cmg init` (writes {}) or pass --config <PATH>.",
                default_config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "~/.config/cmg/config.json".into())
            ),
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config = match path.extension().and_then(|e| e.to_str()) {
            Some("toml") => toml::from_str(&text)
                .with_context(|| format!("parsing TOML config {}", path.display()))?,
            Some("json") => serde_json::from_str(&text)
                .with_context(|| format!("parsing JSON config {}", path.display()))?,
            _ => parse_any(&text).with_context(|| format!("parsing config {}", path.display()))?,
        };
        cfg.validate(&path)?;
        Ok((cfg, path))
    }

    fn validate(&self, path: &Path) -> Result<()> {
        if self.provider.base_url.trim().is_empty() {
            bail!("{}: provider.base_url is empty", path.display());
        }
        if self.provider.model.trim().is_empty() {
            bail!("{}: provider.model is empty", path.display());
        }
        if self.resolve_api_key().is_none() {
            bail!(
                "{}: no API key. Set provider.api_key, or provider.api_key_env to the name of \
                 an environment variable that holds it.",
                path.display()
            );
        }
        Ok(())
    }

    /// Env var takes precedence over the inline key.
    pub fn resolve_api_key(&self) -> Option<String> {
        if let Some(var) = &self.provider.api_key_env {
            if let Ok(val) = std::env::var(var) {
                if !val.trim().is_empty() {
                    return Some(val.trim().to_string());
                }
            }
        }
        self.provider
            .api_key
            .as_ref()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    }
}

fn parse_any(text: &str) -> Result<Config> {
    if let Ok(c) = serde_json::from_str::<Config>(text) {
        return Ok(c);
    }
    Ok(toml::from_str::<Config>(text)?)
}

fn default_config_dir() -> Option<PathBuf> {
    config_dir_from(PLATFORM, |name| std::env::var(name).ok())
}

/// The platform's conventions, as data so each set of rules can be tested on
/// any host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Platform {
    /// `%APPDATA%`-style roaming dir is the native config home on Windows.
    windows: bool,
}

const PLATFORM: Platform = Platform {
    windows: cfg!(windows),
};

/// Resolve the config directory using `get(name)` for the environment.
///
/// Windows has no `$HOME` and no `$XDG_CONFIG_HOME` in PowerShell or cmd, so
/// relying on them made `cmg init` fail outright there. The rules are:
///
/// - Windows: `%APPDATA%\cmg`. `%XDG_CONFIG_HOME%` (set by some tools) then
///   `%HOME%` (set by Git Bash/MSYS) are accepted as fallbacks, and a
///   `%USERPROFILE%\.config` that already exists is honoured, so an existing
///   Git Bash setup keeps working.
/// - Everywhere else: `$XDG_CONFIG_HOME/cmg`, else `$HOME/.config/cmg`.
fn config_dir_from(platform: Platform, get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let var = |name: &str| get(name).filter(|v| !v.trim().is_empty());

    if platform.windows {
        if let Some(appdata) = var("APPDATA") {
            return Some(PathBuf::from(appdata).join("cmg"));
        }
        if let Some(xdg) = var("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(xdg).join("cmg"));
        }
        // Git Bash and MSYS set HOME; honour an existing .config there.
        if let Some(home) = var("HOME") {
            return Some(PathBuf::from(home).join(".config").join("cmg"));
        }
        if let Some(profile) = var("USERPROFILE") {
            return Some(PathBuf::from(profile).join(".config").join("cmg"));
        }
        return None;
    }

    if let Some(xdg) = var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("cmg"));
    }
    var("HOME").map(|h| PathBuf::from(h).join(".config").join("cmg"))
}

/// What to tell the user when no config directory can be determined.
fn missing_config_dir_hint() -> &'static str {
    if cfg!(windows) {
        "cannot determine config directory (no %APPDATA%); set one with --config"
    } else {
        "cannot determine config directory (no $HOME); set one with --config"
    }
}

pub fn default_config_path() -> Option<PathBuf> {
    let dir = default_config_dir()?;
    for name in ["config.json", "config.toml"] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    Some(dir.join("config.json"))
}

fn resolve_path(explicit: Option<&str>) -> Result<Option<PathBuf>> {
    if let Some(p) = explicit {
        let p = PathBuf::from(p);
        if !p.is_file() {
            bail!("config file not found: {}", p.display());
        }
        return Ok(Some(p));
    }
    if let Ok(p) = std::env::var("CMG_CONFIG") {
        if !p.trim().is_empty() {
            let p = PathBuf::from(p);
            if !p.is_file() {
                bail!("$CMG_CONFIG points to a missing file: {}", p.display());
            }
            return Ok(Some(p));
        }
    }
    let dir = match default_config_dir() {
        Some(d) => d,
        None => return Ok(None),
    };
    for name in ["config.json", "config.toml"] {
        let p = dir.join(name);
        if p.is_file() {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

/// Write an example config. Returns the path written.
pub fn write_example(force: bool, toml: bool) -> Result<PathBuf> {
    let dir = default_config_dir().context(missing_config_dir_hint())?;
    let path = dir.join(if toml { "config.toml" } else { "config.json" });
    if path.exists() && !force {
        bail!(
            "{} already exists; pass --force to overwrite",
            path.display()
        );
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let body = if toml { EXAMPLE_TOML } else { EXAMPLE_JSON };
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_parses_with_defaults() {
        let text = r#"{"provider":{"base_url":"http://x/v1","model":"m","api_key":"k"}}"#;
        let c: Config = serde_json::from_str(text).unwrap();
        assert_eq!(c.prompt.max_subject_chars, 72);
        assert!(c.prompt.conventional);
        assert_eq!(c.diff.max_chars, 60_000);
        assert_eq!(c.provider.timeout_secs, 60);
    }

    #[test]
    fn toml_parses_with_defaults() {
        let c: Config = toml::from_str(EXAMPLE_TOML).unwrap();
        assert_eq!(c.provider.model, "gpt-4o-mini");
        assert!(c.output.strip_code_fences);
    }

    #[test]
    fn both_formats_round_trip_via_parse_any() {
        assert!(parse_any(EXAMPLE_JSON).is_ok());
        assert!(parse_any(EXAMPLE_TOML).is_ok());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let text = r#"{"provider":{"base_url":"x","model":"m","api_key":"k"},"nope":1}"#;
        assert!(serde_json::from_str::<Config>(text).is_err());
    }
}

#[cfg(test)]
mod config_dir_tests {
    use super::*;

    const WIN: Platform = Platform { windows: true };
    const UNIX: Platform = Platform { windows: false };

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name: &str| {
            owned
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn windows_uses_appdata_without_home() {
        // The reported bug: PowerShell has no HOME at all.
        let dir = config_dir_from(WIN, env(&[("APPDATA", r"C:\Users\me\AppData\Roaming")]));
        assert_eq!(
            dir,
            Some(PathBuf::from(r"C:\Users\me\AppData\Roaming").join("cmg"))
        );
    }

    #[test]
    fn windows_prefers_appdata_over_git_bash_home() {
        let dir = config_dir_from(
            WIN,
            env(&[
                ("APPDATA", r"C:\Users\me\AppData\Roaming"),
                ("HOME", r"C:\Users\me"),
                ("XDG_CONFIG_HOME", r"C:\xdg"),
            ]),
        );
        assert_eq!(
            dir,
            Some(PathBuf::from(r"C:\Users\me\AppData\Roaming").join("cmg"))
        );
    }

    #[test]
    fn windows_falls_back_to_home_when_appdata_is_missing() {
        let dir = config_dir_from(WIN, env(&[("HOME", r"C:\Users\me")]));
        assert_eq!(
            dir,
            Some(PathBuf::from(r"C:\Users\me").join(".config").join("cmg"))
        );
    }

    #[test]
    fn windows_falls_back_to_userprofile() {
        let dir = config_dir_from(WIN, env(&[("USERPROFILE", r"C:\Users\me")]));
        assert_eq!(
            dir,
            Some(PathBuf::from(r"C:\Users\me").join(".config").join("cmg"))
        );
    }

    #[test]
    fn windows_with_nothing_set_reports_none_not_a_panic() {
        assert_eq!(config_dir_from(WIN, env(&[])), None);
    }

    #[test]
    fn blank_variables_are_ignored() {
        // An empty APPDATA must not produce a path rooted at the cwd.
        let dir = config_dir_from(WIN, env(&[("APPDATA", "   "), ("HOME", r"C:\Users\me")]));
        assert_eq!(
            dir,
            Some(PathBuf::from(r"C:\Users\me").join(".config").join("cmg"))
        );
    }

    #[test]
    fn unix_still_uses_xdg_then_home() {
        let xdg = config_dir_from(
            UNIX,
            env(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/a")]),
        );
        assert_eq!(xdg, Some(PathBuf::from("/xdg").join("cmg")));

        let home = config_dir_from(UNIX, env(&[("HOME", "/home/a")]));
        assert_eq!(
            home,
            Some(PathBuf::from("/home/a").join(".config").join("cmg"))
        );
    }

    #[test]
    fn unix_never_reads_windows_variables() {
        // APPDATA leaking into a Unix environment must not change anything.
        let dir = config_dir_from(UNIX, env(&[("APPDATA", "/nope"), ("HOME", "/home/a")]));
        assert_eq!(
            dir,
            Some(PathBuf::from("/home/a").join(".config").join("cmg"))
        );
    }

    #[test]
    fn unix_with_nothing_set_reports_none() {
        assert_eq!(config_dir_from(UNIX, env(&[])), None);
    }
}
