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
use crossterm::cursor::{self, Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::queue;
use crossterm::style::{Attribute, Print, ResetColor, SetAttribute};
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode, Clear, ClearType};

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
///
/// The menu is drawn relative to a fixed anchor computed once, and every
/// redraw moves to that anchor with an absolute cursor move. Relative moves
/// (`MoveUp`) and saved positions are avoided on purpose: both misbehave once
/// a line has wrapped or scrolled, which made the block crawl across the
/// screen.
fn menu() -> Result<Choice> {
    let mut selected = 0usize;
    let mut stderr = io::stderr();

    // Line wrapping would garble a redraw that exceeds the width, so mask it
    // off for the duration and restore it afterwards.
    enable_raw_mode().context("enabling raw mode (is stderr a terminal?)")?;
    let result = (|| -> Result<Choice> {
        let (_, mut row) = position()?;
        let (_, rows) = terminal::size()?;

        // If the menu would run past the bottom edge, scroll first so every
        // frame starts at a position that still exists on screen.
        let height = (OPTIONS.len() + 1) as u16;
        if row + height >= rows {
            let scroll = row + height - rows + 1;
            queue!(stderr, Print("\n".repeat(scroll as usize)))?;
            row = rows.saturating_sub(height);
            stderr.flush()?;
        }
        let anchor_row = row;

        queue!(stderr, Hide)?;
        loop {
            // Top of the block, column 0, wiping everything below it.
            queue!(
                stderr,
                MoveTo(0, anchor_row),
                Clear(ClearType::FromCursorDown)
            )?;
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
                // Drop the menu and leave the cursor on a clean line below it,
                // so following output does not overwrite the message above.
                queue!(
                    stderr,
                    MoveTo(0, anchor_row),
                    Clear(ClearType::FromCursorDown),
                    MoveTo(0, anchor_row),
                    Show
                )?;
                return Ok(c);
            }
        }
    })();

    let _ = disable_raw_mode();
    let _ = queue!(stderr, terminal::EnableLineWrap, Show);
    let _ = stderr.flush();
    result
}

/// Cursor position and terminal size, falling back to sane values when the
/// terminal does not answer (some multiplexers and CI terminals do not).
fn position() -> Result<(u16, u16)> {
    match cursor::position() {
        Ok(p) => Ok(p),
        Err(_) => {
            let (_cols, rows) = terminal::size().unwrap_or((80, 24));
            Ok((0, rows.saturating_sub(OPTIONS.len() as u16 + 2)))
        }
    }
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
