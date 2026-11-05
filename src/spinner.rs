//! A one-line "working…" indicator for the blocking API call.
//!
//! The request itself is a single blocking call, so the animation has to run
//! on another thread. It is a scoped thread with a stop flag, not an async
//! runtime: nothing here outlives the request.
//!
//! Output goes to stderr and is redrawn in place with `\r` plus a clear-line,
//! the same approach as the menu in `wait.rs`, so it cannot drift. On a plain
//! stderr (`NO_COLOR`, `TERM=dumb`, not a terminal) it prints one static line
//! instead: animation in a log file is just escape-code noise.

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::{
    cursor::MoveToColumn,
    execute, queue,
    terminal::{Clear, ClearType},
};

/// Frame delay. Fast enough to look alive, slow enough to cost nothing.
const TICK: Duration = Duration::from_millis(90);

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// An indicator that animates until it is dropped (or `stop` is called).
pub struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    animated: bool,
}

impl Spinner {
    /// Start showing `label` on stderr while the caller blocks.
    pub fn start(label: &str) -> Self {
        let animated = animation_enabled();
        let mut stderr = io::stderr();

        if !animated {
            // One line, no escape codes: the log stays readable.
            let _ = writeln!(stderr, "cmg: {label}…");
            let _ = stderr.flush();
            return Self {
                stop: Arc::new(AtomicBool::new(false)),
                handle: None,
                animated: false,
            };
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let label = label.to_string();
        let handle = thread::spawn(move || animate(&stop_thread, &label));
        Self {
            stop,
            handle: Some(handle),
            animated: true,
        }
    }

    /// Stop animating and erase the line, leaving the cursor at column 0.
    pub fn stop(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // The thread wakes at most one tick later; joining keeps its final
            // clear-line from racing the message that follows.
            let _ = handle.join();
        }
        let mut stderr = io::stderr();
        if self.animated {
            let _ = queue!(stderr, MoveToColumn(0), Clear(ClearType::CurrentLine));
        }
        let _ = stderr.flush();
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        // Covers the error path: `?` unwinds past the spinner before it can be
        // stopped explicitly.
        self.finish();
    }
}

/// Whether stderr can take a moving line. Mirrors the colour rules in
/// `wait.rs` so a dumb or piped terminal never sees frames.
fn animation_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
        && io::stderr().is_terminal()
}

fn animate(stop: &AtomicBool, label: &str) {
    let mut stderr = io::stderr();
    let started = Instant::now();
    let mut frame = 0usize;

    // Hide the cursor so the redraw does not blink.
    let _ = execute!(stderr, crossterm::cursor::Hide);
    // Wait one tick before the first frame. A request that fails fast (a
    // refused connection is instant) would otherwise flash a spinner and then
    // print the error on the same line.
    if !sleep_until(started + TICK, stop) {
        let _ = execute!(stderr, crossterm::cursor::Show);
        return;
    }
    while !stop.load(Ordering::Relaxed) {
        let elapsed = started.elapsed().as_secs_f64();
        let _ = queue!(
            stderr,
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            SetForegroundColor(Color::DarkGrey),
            Print(format!(
                "  {} {label}… {elapsed:.0}s",
                FRAMES[frame % FRAMES.len()]
            )),
            ResetColor
        );
        let _ = stderr.flush();
        frame += 1;
        sleep_until(Instant::now() + TICK, stop);
    }
    let _ = execute!(stderr, crossterm::cursor::Show);
}

/// Sleep until `deadline`, waking early if `stop` is set. Returns `false` if
/// it was stopped before the deadline.
fn sleep_until(deadline: Instant, stop: &AtomicBool) -> bool {
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
    !stop.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_single_cells() {
        for f in FRAMES {
            assert_eq!(f.chars().count(), 1, "frame {f:?} is not one cell");
        }
    }

    #[test]
    fn a_stopped_spinner_does_not_wait_out_the_tick() {
        // The first frame is gated on one tick; a stop must cut that short so
        // a fast failure never draws a frame.
        let stop = AtomicBool::new(true);
        let started = Instant::now();
        let drew = sleep_until(started + Duration::from_secs(5), &stop);
        assert!(!drew);
        assert!(
            started.elapsed() < TICK * 2,
            "stop was not noticed promptly"
        );
    }

    #[test]
    fn sleep_until_returns_true_when_it_reaches_the_deadline() {
        let stop = AtomicBool::new(false);
        assert!(sleep_until(Instant::now(), &stop));
    }
}
