//! Interactive review loop for `cmg -i`.
//!
//! Everything here talks to stderr: the rendered message, the menu, and the
//! feedback prompt. stdout stays reserved for the final commit message so the
//! "stdout is a contract" rule in AGENTS.md still holds even in interactive mode.
//!
//! Presentation rules, so this stays a CLI and not a TUI:
//! - No box drawing, no alternate screen, no full-screen repaint. The
//!   transcript scrolls away like ordinary command output.
//! - Colour is decorative only. Every distinction is also visible without it
//!   (labels, indentation, bold), and `NO_COLOR` / `TERM=dumb` turn it off.
//! - Only the menu uses raw mode, and only for as long as it is on screen.

use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::{bail, Context, Result};
use crossterm::cursor::{Hide, MoveToColumn, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::queue;
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};

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

/// Whether to emit colour. Honours the usual conventions: `NO_COLOR`, a
/// dumb terminal, or a non-terminal stderr all mean plain text.
pub fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
        && io::stderr().is_terminal()
}

/// Show `message`, ask how to proceed, and read feedback if asked for.
///
/// Only call this when stderr is a terminal; otherwise it would consume piped
/// input.
pub fn review(message: &str) -> Result<Outcome> {
    let mut stderr = io::stderr();
    let color = color_enabled();

    writeln!(stderr)?;
    render_agent_turn(&mut stderr, "suggesting commit message", message, color)?;
    stderr.flush()?;

    match menu(color)? {
        Choice::Commit => Ok(Outcome::Commit),
        Choice::Cancel => Ok(Outcome::Cancel),
        Choice::Regenerate => {
            // Echo the choice as a turn: otherwise the new message appears with
            // no sign of what produced it.
            render_user_action(&mut stderr, "generate another", color)?;
            stderr.flush()?;
            Ok(Outcome::Regenerate)
        }
        Choice::Feedback => match read_feedback(color)? {
            // No echo here: the terminal already showed the line the user
            // typed next to the prompt.
            Some(text) => Ok(Outcome::Feedback(text)),
            None => Ok(Outcome::Cancel),
        },
    }
}

/// Bullet that opens a turn, and the rail that runs down its body.
const BULLET: &str = "●";
const RAIL: &str = "│";
const USER_LABEL: &str = "you ›";

/// Render a turn from the agent: a bullet, a "what it is doing" line, then the
/// reply hanging off a rail.
///
/// The rail is what makes consecutive turns read as one flow instead of a
/// stack of unrelated blocks. It is part of the plain text, so the shape is
/// identical with colour off.
fn render_agent_turn(out: &mut impl Write, action: &str, body: &str, color: bool) -> Result<()> {
    write!(out, "  ")?;
    if color {
        queue!(out, SetForegroundColor(Color::DarkCyan))?;
    }
    write!(out, "{BULLET}")?;
    if color {
        queue!(out, ResetColor)?;
    }
    if color {
        queue!(out, SetForegroundColor(Color::DarkGrey))?;
    }
    writeln!(out, " agent › {action}")?;
    if color {
        queue!(out, ResetColor)?;
    }

    match rail(out, color) {
        Ok(()) => {}
        Err(e) => return Err(e),
    }

    let mut lines = body.lines();
    let first = lines.next().unwrap_or("");
    write_commit_line(out, first, true, color)?;
    for line in lines {
        // Preserve blank lines inside the body, and keep bullet indentation.
        write_commit_line(out, line, false, color)?;
    }
    rail(out, color)?;
    Ok(())
}

/// A user turn that is an action rather than typed text (`generate another`).
/// No rail: there is no body hanging off it.
fn render_user_action(out: &mut impl Write, action: &str, color: bool) -> Result<()> {
    write!(out, "  ")?;
    if color {
        queue!(out, SetForegroundColor(Color::DarkCyan))?;
    }
    writeln!(out, "{BULLET} {USER_LABEL} {action}")?;
    if color {
        queue!(out, ResetColor)?;
    }
    Ok(())
}

/// A rail-only line, closing or continuing a turn.
fn rail(out: &mut impl Write, color: bool) -> Result<()> {
    write!(out, "  ")?;
    if color {
        queue!(out, SetForegroundColor(Color::DarkGrey))?;
        writeln!(out, "{RAIL}")?;
        queue!(out, ResetColor)?;
    } else {
        writeln!(out, "{RAIL}")?;
    }
    Ok(())
}

/// Write one line of a commit message, styling only the Conventional Commits
/// prefix when present. Keeping the subject on one line is intentional: the
/// message is meant to be pasted verbatim, so we never reflow it.
fn write_commit_line(
    out: &mut impl Write,
    line: &str,
    is_subject: bool,
    color: bool,
) -> Result<()> {
    // `│  ` keeps every body line inside the turn's rail. A blank line gets
    // the rail alone, with no trailing spaces.
    if !is_subject {
        if line.trim().is_empty() {
            writeln!(out, "  {RAIL}")?;
        } else {
            writeln!(out, "  {RAIL}  {line}")?;
        }
        return Ok(());
    }

    match split_prefix(line) {
        Some((prefix, rest)) if color => {
            write!(out, "  {RAIL}  ")?;
            let _ = queue!(out, SetAttribute(Attribute::Bold));
            write!(out, "{prefix}")?;
            let _ = queue!(out, SetAttribute(Attribute::Reset));
            writeln!(out, "{rest}")?;
        }
        _ => writeln!(out, "  {RAIL}  {line}")?,
    }
    Ok(())
}

/// Split `type(scope)!: rest` into the styled prefix and the remainder.
/// Returns `None` for anything that is not a Conventional Commits subject.
fn split_prefix(line: &str) -> Option<(String, &str)> {
    let colon = line.find(": ")?;
    let head = &line[..colon + 1];
    let body = &line[colon + 1..];

    let type_end = head.find(['(', '!', ':'])?;
    let ty = &head[..type_end];
    if ty.is_empty() || !ty.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    Some((head.to_string(), body))
}

/// Arrow-key menu. Returns the chosen option, or `Cancel` on Esc/Ctrl-C.
///
/// The menu is a single line, redrawn in place with a carriage return plus a
/// clear-line. That is deliberately the dumbest possible redraw: it needs no
/// cursor query, no saved position, no line counting and no absolute
/// coordinates, so it cannot drift when a line wraps, the screen scrolls, or
/// the terminal misreports where the cursor is.
fn menu(color: bool) -> Result<Choice> {
    let mut selected = 0usize;
    let mut stderr = io::stderr();

    enable_raw_mode().context("enabling raw mode (is stderr a terminal?)")?;
    let result = (|| -> Result<Choice> {
        queue!(stderr, Hide)?;
        loop {
            // A carriage return goes back to column 0, then the line is wiped:
            // no state is carried over between frames.
            queue!(stderr, MoveToColumn(0), Clear(ClearType::CurrentLine))?;

            // Same two-space margin as the turns, so the menu lines up under
            // the rail instead of floating at its own indent.
            queue!(stderr, Print("  "))?;
            for (i, (label, _)) in OPTIONS.iter().enumerate() {
                if i > 0 {
                    queue!(stderr, Print("   "))?;
                }
                if i == selected {
                    if color {
                        queue!(
                            stderr,
                            SetAttribute(Attribute::Reverse),
                            SetForegroundColor(Color::White),
                            Print(format!(" {label} ")),
                            ResetColor,
                            SetAttribute(Attribute::Reset)
                        )?;
                    } else {
                        // Without colour the selection must still be visible.
                        queue!(
                            stderr,
                            SetAttribute(Attribute::Reverse),
                            Print(format!(" {label} ")),
                            SetAttribute(Attribute::Reset)
                        )?;
                    }
                } else {
                    queue!(stderr, Print(format!(" {label} ")))?;
                }
            }

            if color {
                queue!(stderr, SetForegroundColor(Color::DarkGrey))?;
            }
            queue!(stderr, Print("  ←/→ move · enter select · esc cancel"))?;
            if color {
                queue!(stderr, ResetColor)?;
            }
            stderr.flush()?;

            let choice = match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    match (key.code, key.modifiers) {
                        (KeyCode::Left, _)
                        | (KeyCode::Up, _)
                        | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                            selected = selected.checked_sub(1).unwrap_or(OPTIONS.len() - 1);
                            None
                        }
                        (KeyCode::Right, _)
                        | (KeyCode::Down, _)
                        | (KeyCode::Tab, _)
                        | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                            selected = (selected + 1) % OPTIONS.len();
                            None
                        }
                        (KeyCode::Enter, _) => Some(OPTIONS[selected].1),
                        // First letters select directly; vim keys move.
                        (KeyCode::Char('c'), KeyModifiers::NONE) => Some(Choice::Commit),
                        (KeyCode::Char('g'), KeyModifiers::NONE) => Some(Choice::Regenerate),
                        (KeyCode::Char('f'), KeyModifiers::NONE) => Some(Choice::Feedback),
                        (KeyCode::Esc, _) => Some(Choice::Cancel),
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Choice::Cancel),
                        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(Choice::Cancel),
                        _ => None,
                    }
                }
                _ => None,
            };

            if let Some(c) = choice {
                // Erase the menu and start a fresh line below it.
                queue!(
                    stderr,
                    MoveToColumn(0),
                    Clear(ClearType::CurrentLine),
                    Print("\n"),
                    Show
                )?;
                return Ok(c);
            }
        }
    })();

    let _ = disable_raw_mode();
    let _ = queue!(stderr, Show);
    let _ = stderr.flush();
    result
}

/// Read one line of feedback. Empty input means "no change", which we treat
/// as a cancel so the caller does not burn a request.
fn read_feedback(color: bool) -> Result<Option<String>> {
    let mut stderr = io::stderr();
    write!(stderr, "  ")?;
    if color {
        queue!(stderr, SetForegroundColor(Color::DarkCyan))?;
    }
    write!(stderr, "{BULLET} {USER_LABEL}")?;
    if color {
        queue!(stderr, ResetColor)?;
    }
    write!(stderr, " ")?;
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
    fn conventional_prefix_is_split() {
        assert_eq!(
            split_prefix("feat(blog): add page").map(|(p, r)| (p, r.to_string())),
            Some(("feat(blog):".to_string(), " add page".to_string()))
        );
        assert!(split_prefix("fix!: drop x").is_some());
    }

    #[test]
    fn non_conventional_subjects_are_left_alone() {
        assert!(split_prefix("updated the readme").is_none());
        assert!(split_prefix("Fix stuff: later").is_none());
        assert!(split_prefix("no colon here").is_none());
    }

    #[test]
    fn plain_rendering_has_no_escape_codes() {
        let mut buf = Vec::new();
        render_agent_turn(
            &mut buf,
            "suggesting commit message",
            "feat: x\n\nbody",
            false,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(!s.contains('\u{1b}'), "plain output leaked escapes: {s:?}");
        assert!(s.contains("agent › suggesting commit message"));
        assert!(s.contains("feat: x"));
    }
}

#[cfg(test)]
mod plain_tests {
    use super::*;

    #[test]
    fn agent_turn_shape_survives_without_colour() {
        let mut buf = Vec::new();
        render_agent_turn(
            &mut buf,
            "suggesting commit message",
            "feat: x\n\nbody",
            false,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(
            s,
            "  ● agent › suggesting commit message\n  │\n  │  feat: x\n  │\n  │  body\n  │\n"
        );
        assert!(!s.contains('\u{1b}'));
    }

    #[test]
    fn plain_turn_keeps_blank_lines_inside_the_rail() {
        let mut buf = Vec::new();
        render_agent_turn(&mut buf, "revising", "fix: y\n\n- one\n\n- two", false).unwrap();
        let s = String::from_utf8(buf).unwrap();
        // A blank line in the body must not break the rail.
        for line in s.lines().skip(1) {
            assert!(line.starts_with("  │"), "line broke the rail: {line:?}");
        }
    }
}

#[cfg(test)]
mod user_turn_tests {
    use super::*;

    #[test]
    fn regenerate_is_echoed_as_a_user_turn() {
        let mut buf = Vec::new();
        render_user_action(&mut buf, "generate another", false).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "  ● you › generate another\n"
        );
    }
}
