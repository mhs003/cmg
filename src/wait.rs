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

use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result};
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

    // The menu is re-entered after a feedback prompt is abandoned, so Ctrl-C
    // there means "back", not "quit the whole run".
    loop {
        match menu(color)? {
            Choice::Commit => return Ok(Outcome::Commit),
            Choice::Cancel => return Ok(Outcome::Cancel),
            Choice::Regenerate => {
                // Echo the choice as a turn: otherwise the new message appears
                // with no sign of what produced it.
                render_user_action(&mut stderr, "generate another", color)?;
                stderr.flush()?;
                return Ok(Outcome::Regenerate);
            }
            Choice::Feedback => match read_feedback(color)? {
                // No echo here: the terminal already showed the line the user
                // typed next to the prompt.
                Some(text) => return Ok(Outcome::Feedback(text)),
                // Ctrl-C / Esc in the editor: drop the half-typed line and
                // return to the choices with the current message untouched.
                None => continue,
            },
        }
    }
}

/// Bullet that opens a turn, and the rail that runs down its body.
const BULLET: &str = "●";
const RAIL: &str = "│";
const USER_LABEL: &str = "you ›";
/// What the feedback prompt looks like before any typing.
const PROMPT_PREFIX: &str = "  ● you ›";

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

/// Read one line of feedback with line editing.
///
/// A plain `read_line` cannot do this: in cooked mode the arrows, Home/End and
/// Ctrl/Alt+word bindings arrive as literal escape bytes (`^[[D`). Raw mode is
/// enabled only while the prompt is up, then always restored.
fn read_feedback(_color: bool) -> Result<Option<String>> {
    // The prompt is drawn by the first redraw below, so there is one owner of
    // what is on screen. `None` means the prompt was abandoned (Ctrl-C, Esc, or
    // an empty line): the caller goes back to the menu.
    let Some(line) = edit_line()? else {
        return Ok(None);
    };
    Ok(if line.trim().is_empty() {
        None
    } else {
        Some(line.trim().to_string())
    })
}

/// Line-editing state. Separate from the terminal so every movement rule can
/// be tested without a TTY.
#[derive(Default)]
struct LineEditor {
    chars: Vec<char>,
    /// Cursor position as a char index, 0..=len.
    at: usize,
}

impl LineEditor {
    fn insert(&mut self, c: char) {
        self.chars.insert(self.at, c);
        self.at += 1;
    }

    fn backspace(&mut self) {
        if self.at > 0 {
            self.at -= 1;
            self.chars.remove(self.at);
        }
    }

    /// Ctrl-W / Alt-Backspace: delete the word before the cursor, including
    /// the whitespace that precedes it, like a shell.
    fn delete_word_back(&mut self) {
        while self.at > 0 && self.chars[self.at - 1].is_whitespace() {
            self.at -= 1;
            self.chars.remove(self.at);
        }
        while self.at > 0 && !self.chars[self.at - 1].is_whitespace() {
            self.at -= 1;
            self.chars.remove(self.at);
        }
    }

    /// Del key: delete the character under the cursor.
    fn delete_forward(&mut self) {
        if self.at < self.chars.len() {
            self.chars.remove(self.at);
        }
    }

    /// Ctrl-K: delete from the cursor to the end of the line.
    fn kill_to_end(&mut self) {
        self.chars.truncate(self.at);
    }

    /// Ctrl-U: delete from the start of the line to the cursor.
    fn kill_to_start(&mut self) {
        self.chars.drain(..self.at);
        self.at = 0;
    }

    fn left(&mut self) {
        self.at = self.at.saturating_sub(1);
    }

    fn right(&mut self) {
        if self.at < self.chars.len() {
            self.at += 1;
        }
    }

    /// Alt-Left / Alt-B: move to the start of the previous word.
    fn word_left(&mut self) {
        while self.at > 0 && self.chars[self.at - 1].is_whitespace() {
            self.at -= 1;
        }
        while self.at > 0 && !self.chars[self.at - 1].is_whitespace() {
            self.at -= 1;
        }
    }

    /// Alt-Right / Alt-F: move past the end of the current word.
    fn word_right(&mut self) {
        let len = self.chars.len();
        while self.at < len && self.chars[self.at].is_whitespace() {
            self.at += 1;
        }
        while self.at < len && !self.chars[self.at].is_whitespace() {
            self.at += 1;
        }
    }

    fn home(&mut self) {
        self.at = 0;
    }

    fn end(&mut self) {
        self.at = self.chars.len();
    }

    fn text(&self) -> String {
        self.chars.iter().collect()
    }
}

/// Outcome of one key press.
enum Edit {
    /// Keep editing.
    Continue,
    /// The user accepted the line.
    Accept,
    /// The user abandoned the line (Esc / Ctrl-C).
    Cancel,
}

/// Redraw one line in place: carriage return, wipe, prompt, text, then place
/// the cursor. Single-line only, so it needs no line counting.
fn redraw_line(out: &mut impl Write, prefix: &str, editor: &LineEditor, color: bool) -> Result<()> {
    queue!(
        out,
        MoveToColumn(0),
        Clear(ClearType::CurrentLine),
        Print(prefix)
    )?;
    if color {
        queue!(out, SetForegroundColor(Color::DarkCyan))?;
    }
    queue!(out, Print(editor.text()))?;
    if color {
        queue!(out, ResetColor)?;
    }
    // Back up from the end of the text to the cursor.
    let back = editor.chars.len() - editor.at;
    if back > 0 {
        queue!(out, crossterm::cursor::MoveLeft(back as u16))?;
    }
    out.flush()?;
    Ok(())
}

/// Read a line with arrow-key editing.
///
/// `Ok(None)` means the user abandoned the line (Ctrl-C / Esc / Ctrl-D on an
/// empty line). That is a normal outcome, not an error: the caller decides
/// whether it means "go back" or "quit".
fn edit_line() -> Result<Option<String>> {
    let mut editor = LineEditor::default();
    let mut stderr = io::stderr();
    // Exactly what the prompt looks like, so the first redraw is a clean
    // in-place replacement rather than a second prompt.
    let prefix = format!("{PROMPT_PREFIX} ");

    enable_raw_mode().context("enabling raw mode (is stderr a terminal?)")?;
    let result = (|| -> Result<Option<String>> {
        loop {
            redraw_line(&mut stderr, &prefix, &editor, color_enabled())?;
            let edit = match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    apply_key(&mut editor, key.code, key.modifiers)
                }
                // Ignore mouse, resize, focus and paste events.
                _ => Edit::Continue,
            };
            match edit {
                Edit::Continue => {}
                Edit::Accept => {
                    // Leave the finished line on screen and move below it.
                    redraw_line(
                        &mut stderr,
                        &prefix,
                        &editor_with_cursor_at_end(&editor),
                        color_enabled(),
                    )?;
                    queue!(stderr, Print("\r\n"))?;
                    stderr.flush()?;
                    return Ok(Some(editor.text()));
                }
                Edit::Cancel => {
                    // Move off the half-typed line so the menu redraws below
                    // it instead of overwriting it in place.
                    queue!(stderr, Print("\r\n"))?;
                    stderr.flush()?;
                    return Ok(None);
                }
            }
        }
    })();
    let _ = disable_raw_mode();
    result
}

/// The accepted line is redrawn with the cursor at the end so it does not sit
/// mid-text after Enter.
fn editor_with_cursor_at_end(e: &LineEditor) -> LineEditor {
    LineEditor {
        chars: e.chars.clone(),
        at: e.chars.len(),
    }
}

fn apply_key(editor: &mut LineEditor, code: KeyCode, mods: KeyModifiers) -> Edit {
    // Alt+Backspace / Alt+arrow move or delete by word, as in readline.
    let word = mods.contains(KeyModifiers::ALT);
    match code {
        KeyCode::Enter => return Edit::Accept,
        KeyCode::Esc => return Edit::Cancel,
        KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => return Edit::Cancel,
        KeyCode::Char('d') if mods.contains(KeyModifiers::CONTROL) && editor.chars.is_empty() => {
            return Edit::Cancel
        }
        KeyCode::Char('a') if mods.contains(KeyModifiers::CONTROL) => editor.home(),
        KeyCode::Char('e') if mods.contains(KeyModifiers::CONTROL) => editor.end(),
        KeyCode::Char('b') if mods.contains(KeyModifiers::CONTROL) => editor.left(),
        KeyCode::Char('f') if mods.contains(KeyModifiers::CONTROL) => editor.right(),
        KeyCode::Char('w') if mods.contains(KeyModifiers::CONTROL) => editor.delete_word_back(),
        KeyCode::Char('k') if mods.contains(KeyModifiers::CONTROL) => editor.kill_to_end(),
        KeyCode::Char('u') if mods.contains(KeyModifiers::CONTROL) => editor.kill_to_start(),
        KeyCode::Char(_) if mods.contains(KeyModifiers::CONTROL) => {}
        KeyCode::Char(c) => editor.insert(c),
        KeyCode::Backspace if word => editor.delete_word_back(),
        KeyCode::Backspace => editor.backspace(),
        KeyCode::Delete => editor.delete_forward(),
        KeyCode::Left if word => editor.word_left(),
        KeyCode::Left => editor.left(),
        KeyCode::Right if word => editor.word_right(),
        KeyCode::Right => editor.right(),
        KeyCode::Home => editor.home(),
        KeyCode::End => editor.end(),
        _ => {}
    }
    Edit::Continue
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

#[cfg(test)]
mod editor_tests {
    use super::*;

    fn typed(s: &str) -> LineEditor {
        let mut e = LineEditor::default();
        for c in s.chars() {
            e.insert(c);
        }
        e
    }

    #[test]
    fn typing_inserts_at_the_cursor_not_the_end() {
        let mut e = typed("ac");
        e.left();
        e.insert('b');
        assert_eq!(e.text(), "abc");
        assert_eq!(e.at, 2);
    }

    #[test]
    fn arrows_move_and_clamp_at_the_edges() {
        let mut e = typed("abc");
        e.left();
        e.left();
        e.left();
        e.left(); // already at the start
        assert_eq!(e.at, 0);
        e.end();
        e.right(); // already at the end
        assert_eq!(e.at, 3);
    }

    #[test]
    fn home_and_end_jump() {
        let mut e = typed("hello");
        e.home();
        assert_eq!(e.at, 0);
        e.end();
        assert_eq!(e.at, 5);
    }

    #[test]
    fn backspace_deletes_before_the_cursor() {
        let mut e = typed("abc");
        e.left();
        e.backspace();
        assert_eq!(e.text(), "ac");
        assert_eq!(e.at, 1);
    }

    #[test]
    fn backspace_at_the_start_does_nothing() {
        let mut e = typed("ab");
        e.home();
        e.backspace();
        assert_eq!(e.text(), "ab");
        assert_eq!(e.at, 0);
    }

    #[test]
    fn delete_removes_the_character_under_the_cursor() {
        let mut e = typed("abc");
        e.home();
        e.delete_forward();
        assert_eq!(e.text(), "bc");
        assert_eq!(e.at, 0);
    }

    #[test]
    fn cursor_movement_keycodes_are_wired_up() {
        let mut e = typed("one two");
        // Ctrl-A / Ctrl-E
        apply_key(&mut e, KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(e.at, 0);
        apply_key(&mut e, KeyCode::Char('e'), KeyModifiers::CONTROL);
        assert_eq!(e.at, 7);
        // Left/Right
        apply_key(&mut e, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(e.at, 6);
        apply_key(&mut e, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(e.at, 0);
    }

    #[test]
    fn alt_arrows_move_by_word() {
        let mut e = typed("one two three");
        apply_key(&mut e, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(e.at, 8, "should land at the start of \"three\"");
        apply_key(&mut e, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(e.at, 4, "should land at the start of \"two\"");
        apply_key(&mut e, KeyCode::Right, KeyModifiers::ALT);
        assert_eq!(e.at, 7, "should land past the end of \"two\"");
    }

    #[test]
    fn alt_backspace_and_ctrl_w_delete_a_word() {
        let mut e = typed("fix the parser");
        apply_key(&mut e, KeyCode::Backspace, KeyModifiers::ALT);
        assert_eq!(e.text(), "fix the ");
        apply_key(&mut e, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(e.text(), "fix ");
        apply_key(&mut e, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(e.text(), "");
    }

    #[test]
    fn ctrl_k_and_ctrl_u_kill_to_the_edges() {
        let mut e = typed("hello world");
        e.at = 5;
        e.kill_to_end();
        assert_eq!(e.text(), "hello");
        e.kill_to_start();
        assert_eq!(e.text(), "");
        assert_eq!(e.at, 0);
    }

    #[test]
    fn enter_accepts_and_escape_cancels() {
        let mut e = typed("x");
        assert!(matches!(
            apply_key(&mut e, KeyCode::Enter, KeyModifiers::NONE),
            Edit::Accept
        ));
        assert!(matches!(
            apply_key(&mut e, KeyCode::Esc, KeyModifiers::NONE),
            Edit::Cancel
        ));
        assert!(matches!(
            apply_key(&mut e, KeyCode::Char('c'), KeyModifiers::CONTROL),
            Edit::Cancel
        ));
    }

    #[test]
    fn ctrl_d_cancels_only_on_an_empty_line() {
        let mut e = LineEditor::default();
        assert!(matches!(
            apply_key(&mut e, KeyCode::Char('d'), KeyModifiers::CONTROL),
            Edit::Cancel
        ));
        let mut e = typed("text");
        assert!(matches!(
            apply_key(&mut e, KeyCode::Char('d'), KeyModifiers::CONTROL),
            Edit::Continue
        ));
        assert_eq!(e.text(), "text");
    }

    #[test]
    fn cursor_is_reported_in_chars_not_bytes() {
        // A multi-byte glyph must not shift the cursor by its byte length.
        let mut e = typed("héllo");
        e.home();
        e.right();
        e.right();
        assert_eq!(e.at, 2);
        e.insert('X');
        assert_eq!(e.text(), "héXllo");
    }
}
