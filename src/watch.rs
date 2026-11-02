//! Interactive review loop for `cmg -w`.
//!
//! Everything here talks to stderr: the rendered message, the menu, and the
//! feedback prompt. stdout stays reserved for the final commit message so the
//! "stdout is a contract" rule in AGENTS.md still holds even in watch mode.
//!
//! The menu uses arrow keys (raw mode); feedback is a plain line read so it
//! behaves like any other terminal input, including backspace and Ctrl-C.

use std::io::{self, BufRead, Write};

use anyhow::{bail, Context, Result};
use crossterm::cursor::{Hide, MoveDown, MoveToColumn, RestorePosition, SavePosition, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::queue;
use crossterm::style::{Attribute, Print, ResetColor, SetAttribute};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, DisableLineWrap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Commit,
    Regenerate,
    Feedback,
    Cancel,
}

const OPTIONS: [(&str, Choice); 4] = [
    ("commit", Choice::Commit),
    ("generate another", Choice::Regenerate),
    ("feedback", Choice::Feedback),
    ("cancel", Choice::Cancel),
];

/// What the review loop produced.
pub enum Outcome {
    Commit,
    Cancel,
    /// The user asked for a fresh message from the same diff.
    Regenerate,
    /// The user typed feedback that the message should be revised with.
    Feedback(String),
}

/// Show `message`, ask how to proceed, and read feedback if asked for.
///
/// Only call this when stderr is a terminal; otherwise it would consume piped
/// input.
pub fn review(message: &str) -> Result<Outcome> {
    let mut stderr = io::stderr();
    writeln!(stderr, "\n{}", indent(message))?;
    stderr.flush()?;

    match menu()? {
        Choice::Commit => Ok(Outcome::Commit),
        Choice::Cancel => Ok(Outcome::Cancel),
        Choice::Regenerate => Ok(Outcome::Regenerate),
        Choice::Feedback => match read_feedback()? {
            Some(text) => Ok(Outcome::Feedback(text)),
            None => Ok(Outcome::Cancel),
        },
    }
}

fn indent(message: &str) -> String {
    message
        .lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Arrow-key menu. Returns the chosen option, or `Cancel` on Esc/Ctrl-C.
fn menu() -> Result<Choice> {
    let mut selected = 0usize;
    let mut stderr = io::stderr();

    // Line wrapping would garble a redraw that exceeds the width, so mask it
    // off for the duration and restore it afterwards.
    enable_raw_mode().context("enabling raw mode (is stderr a terminal?)")?;
    let result = (|| -> Result<Choice> {
        queue!(stderr, DisableLineWrap)?;
        // Saved before the first frame, restored on every redraw: counting
        // MoveUp() lines drifts because the cursor ends up mid-line.
        queue!(stderr, SavePosition, Hide)?;
        loop {
            queue!(stderr, RestorePosition, Clear(ClearType::FromCursorDown))?;
            for (i, (label, _)) in OPTIONS.iter().enumerate() {
                if i == selected {
                    queue!(
                        stderr,
                        SetAttribute(Attribute::Reverse),
                        Print(format!(" > {label}")),
                        ResetColor,
                        SetAttribute(Attribute::Reset)
                    )?;
                } else {
                    queue!(stderr, Print(format!("   {label}")))?;
                }
                queue!(stderr, Print("\n"))?;
            }
            queue!(stderr, Print(" (arrows/enter, esc to cancel)"))?;
            stderr.flush()?;

            let choice = match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    match (key.code, key.modifiers) {
                        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                            selected = selected.checked_sub(1).unwrap_or(OPTIONS.len() - 1);
                            None
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                            selected = (selected + 1) % OPTIONS.len();
                            None
                        }
                        (KeyCode::Enter, _) => Some(OPTIONS[selected].1),
                        (KeyCode::Esc, _) => Some(Choice::Cancel),
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Choice::Cancel),
                        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(Choice::Cancel),
                        _ => None,
                    }
                }
                _ => None,
            };

            if let Some(c) = choice {
                // Leave the cursor on the line below the cleared menu so later
                // output does not overwrite the message above it.
                queue!(stderr, RestorePosition, Clear(ClearType::FromCursorDown))?;
                let lines = OPTIONS.len() + 1;
                queue!(stderr, MoveDown(lines as u16), MoveToColumn(0))?;
                queue!(stderr, Show)?;
                return Ok(c);
            }
        }
    })();

    let _ = disable_raw_mode();
    let _ = queue!(stderr, crossterm::terminal::EnableLineWrap, Show);
    let _ = stderr.flush();
    result
}

/// Read one line of feedback. Empty input means "no change", which we treat
/// as a cancel so the caller does not burn a request.
fn read_feedback() -> Result<Option<String>> {
    let mut stderr = io::stderr();
    write!(stderr, "feedback (empty to cancel): ")?;
    stderr.flush()?;

    let mut line = String::new();
    let stdin = io::stdin();
    match stdin.lock().read_line(&mut line) {
        Ok(0) => bail!("no feedback given (stdin closed)"),
        Ok(_) => {}
        Err(e) => bail!("reading feedback: {e}"),
    }
    let text = line.trim().to_string();
    Ok(if text.is_empty() { None } else { Some(text) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_has_the_four_options_in_order() {
        let labels: Vec<&str> = OPTIONS.iter().map(|(l, _)| *l).collect();
        assert_eq!(
            labels,
            vec!["commit", "generate another", "feedback", "cancel"]
        );
        assert_eq!(OPTIONS[0].1, Choice::Commit);
        assert_eq!(OPTIONS[3].1, Choice::Cancel);
    }

    #[test]
    fn indent_prefixes_every_line() {
        assert_eq!(indent("a\nb"), "  a\n  b");
    }
}
