//! End-to-end tests against a stub OpenAI-compatible server.
//!
//! No network and no API key: the stub listens on 127.0.0.1 and returns a
//! canned response. Tests are ignored by default (`--ignored`) because they
//! require `git` and a built binary; run with `cargo test -- --ignored`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

/// Spawn a one-shot HTTP server. Returns its base URL and a handle to the
/// request body it received.
fn stub_server(response_body: &'static str) -> (String, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(String::new()));
    let seen_clone = seen.clone();

    thread::spawn(move || {
        for stream in listener.incoming().take(1) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body).unwrap();
            *seen_clone.lock().unwrap() = String::from_utf8_lossy(&body).into_owned();

            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });

    (format!("http://{addr}/v1"), seen)
}

fn cmg_bin() -> PathBuf {
    // target/debug/cmg next to the test binary.
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("cmg")
}

struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cmg-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repo { dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "t@example.com"]);
        repo.git(&["config", "user.name", "Test"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn write(&self, name: &str, contents: &str) {
        let p = self.dir.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, contents).unwrap();
    }

    fn write_config(&self, base_url: &str) -> PathBuf {
        let path = self.dir.join("cmg.json");
        let cfg = format!(
            r#"{{"provider":{{"base_url":"{base_url}","model":"test-model","api_key":"sk-test"}},
                "prompt":{{"include_log_context":false}}}}"#
        );
        std::fs::write(&path, cfg).unwrap();
        path
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(cmg_bin())
            .args(args)
            .current_dir(&self.dir)
            .env_remove("CMG_CONFIG")
            .output()
            .unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const OK_RESPONSE: &str =
    r#"{"choices":[{"message":{"role":"assistant","content":"```\nfeat: add greeter\n```"}}]}"#;

#[test]
#[ignore]
fn prints_cleaned_message_to_stdout() {
    let (url, seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("stdout");
    repo.write("a.txt", "hello\n");
    repo.git(&["add", "a.txt"]);
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--config", cfg.to_str().unwrap()]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "feat: add greeter\n");
    let body = seen.lock().unwrap();
    assert!(body.contains("test-model"));
    assert!(body.contains("+hello"));
}

#[test]
#[ignore]
fn staged_wins_over_unstaged() {
    let (url, seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("staged-wins");
    repo.write("staged.txt", "staged\n");
    repo.git(&["add", "staged.txt"]);
    repo.write("other.txt", "unstaged\n");
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--config", cfg.to_str().unwrap()]);
    assert!(out.status.success());

    let body = seen.lock().unwrap();
    assert!(body.contains("staged.txt"));
    assert!(!body.contains("other.txt"));
}

#[test]
#[ignore]
fn falls_back_to_unstaged() {
    let (url, seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("fallback");
    repo.write("a.txt", "one\n");
    repo.git(&["add", "a.txt"]);
    repo.git(&["commit", "-qm", "init"]);
    repo.write("a.txt", "two\n");
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--config", cfg.to_str().unwrap()]);
    assert!(out.status.success());

    let body = seen.lock().unwrap();
    assert!(body.contains("+two"));
}

#[test]
#[ignore]
fn commit_flag_creates_commit() {
    let (url, _seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("commit");
    repo.write("a.txt", "x\n");
    repo.git(&["add", "a.txt"]);
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--commit", "--config", cfg.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        repo.git(&["log", "-1", "--pretty=%s"]).trim(),
        "feat: add greeter"
    );
}

#[test]
#[ignore]
fn commit_flag_stages_unstaged_edits() {
    let (url, _seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("commit-unstaged");
    repo.write("a.txt", "one\n");
    repo.git(&["add", "a.txt"]);
    repo.git(&["commit", "-qm", "init"]);
    repo.write("a.txt", "two\n");
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--commit", "--config", cfg.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(repo.git(&["status", "--porcelain"]).trim(), "");
}

#[test]
#[ignore]
fn untracked_only_generates_message() {
    let (url, seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("untracked");
    repo.write("new.txt", "hello untracked\n");
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--config", cfg.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "feat: add greeter\n");

    let body = seen.lock().unwrap();
    assert!(body.contains("new.txt"), "prompt lacks the file name");
    assert!(
        body.contains("+hello untracked"),
        "prompt lacks the content"
    );
}

#[test]
#[ignore]
fn untracked_empty_files_still_list_names() {
    let (url, seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("untracked-empty");
    repo.write("placeholder", "");
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--config", cfg.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(seen.lock().unwrap().contains("placeholder"));
}

#[test]
#[ignore]
fn untracked_commit_stages_and_commits() {
    let (url, _seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("untracked-commit");
    repo.write("new.txt", "x\n");
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--commit", "--config", cfg.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        repo.git(&["log", "-1", "--pretty=%s"]).trim(),
        "feat: add greeter"
    );
    assert_eq!(repo.git(&["status", "--porcelain"]).trim(), "");
}

#[test]
#[ignore]
fn untracked_respects_path_filter() {
    let (url, seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("untracked-path");
    repo.write("keep/a.txt", "a\n");
    repo.write("skip/b.txt", "b\n");
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--path", "keep", "--config", cfg.to_str().unwrap()]);
    assert!(out.status.success());

    let body = seen.lock().unwrap();
    assert!(body.contains("keep/a.txt"));
    assert!(!body.contains("skip/b.txt"));
}

#[test]
#[ignore]
fn missing_api_key_is_a_config_error() {
    let repo = Repo::new("nokey");
    repo.write("a.txt", "x\n");
    repo.git(&["add", "a.txt"]);
    let path = repo.dir.join("bad.json");
    std::fs::write(
        &path,
        r#"{"provider":{"base_url":"http://127.0.0.1:1/v1","model":"m"}}"#,
    )
    .unwrap();

    let out = repo.run(&["--config", path.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no API key"));
}

#[test]
#[ignore]
fn root_commit_works() {
    let (url, seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("root");
    repo.write("first.txt", "hi\n");
    repo.git(&["add", "first.txt"]);
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--config", cfg.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(seen.lock().unwrap().contains("+hi"));
}

#[test]
#[ignore]
fn the_indicator_never_reaches_stdout() {
    // Every mode shows a "thinking" line on stderr; stdout must stay exactly
    // the message so `git commit -m "$(cmg)"` works.
    let (url, _seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("stdout-contract");
    repo.write("a.txt", "x\n");
    repo.git(&["add", "a.txt"]);
    let cfg = repo.write_config(&url);

    let out = repo.run(&["--config", cfg.to_str().unwrap()]);
    assert!(out.status.success());

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("thinking"), "indicator leaked: {stdout:?}");
    assert_eq!(stdout.trim(), "feat: add greeter");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("thinking"), "stderr: {stderr:?}");
}

#[test]
#[ignore]
fn legacy_flag_spellings_warn_on_stderr() {
    let repo = Repo::new("interactive-deprecated");
    repo.write("a.txt", "x\n");

    for flag in ["-w", "--wait", "--watch"] {
        let out = Command::new(cmg_bin())
            .arg(flag)
            .current_dir(&repo.dir)
            .env_remove("CMG_CONFIG")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains(&format!("`{flag}` is deprecated")),
            "{flag} should warn, got: {err}"
        );
        assert!(err.contains("-i"), "{flag} warning should name -i: {err}");
        // The warning is noise, so it must never reach stdout.
        assert!(out.stdout.is_empty(), "{flag} wrote to stdout");
    }

    // The current spelling must stay quiet.
    let out = Command::new(cmg_bin())
        .arg("-i")
        .current_dir(&repo.dir)
        .env_remove("CMG_CONFIG")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("deprecated"), "-i should not warn: {err}");
}

#[test]
#[ignore]
fn interactive_refuses_without_a_terminal() {
    let (url, _seen) = stub_server(OK_RESPONSE);
    let repo = Repo::new("interactive-notty");
    repo.write("a.txt", "x\n");
    repo.git(&["add", "a.txt"]);
    let cfg = repo.write_config(&url);

    // Tests run with piped stdio, which is exactly the case we reject.
    let out = repo.run(&["-w", "--config", cfg.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs a terminal"));
}

#[test]
#[ignore]
fn wait_and_commit_are_independent_flags() {
    let out = Command::new(cmg_bin()).arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("-i, --interactive"), "help: {help}");
    assert!(help.contains("-c, --commit"), "help: {help}");
}

#[test]
#[ignore]
fn interactive_keeps_the_old_flag_aliases() {
    // The flag was renamed; keep the old spelling working.
    let repo = Repo::new("interactive-alias");
    repo.write("a.txt", "x\n");
    for flag in ["-w", "--wait", "--watch", "-i", "--interactive"] {
        let out = Command::new(cmg_bin())
            .arg(flag)
            .current_dir(&repo.dir)
            .env_remove("CMG_CONFIG")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("needs a terminal"),
            "{flag} should reach interactive mode, got: {err}"
        );
    }
}
