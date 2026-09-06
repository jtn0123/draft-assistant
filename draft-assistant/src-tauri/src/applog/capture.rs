//! Reading back what was logged, for tests.
//!
//! `DIR` is a `OnceLock` no test may set -- one test claiming it would wedge
//! it for every other test in the binary -- so this is how a test reads what
//! its code actually wrote.
//!
//! Per thread, not per process. It used to be one buffer for the whole test
//! binary behind a gate, and a test asserting its command wrote nothing at
//! all failed whenever a test on another thread logged a warning in the same
//! instant: the other thread's line landed in this one's buffer. Each thread
//! now has its own sink, so a capture sees exactly what its own test wrote and
//! two tests capturing at once no longer wait for each other.

use std::cell::RefCell;
use std::marker::PhantomData;

thread_local! {
    /// This thread's open capture, or `None` when lines go to the file.
    static CAPTURED: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// Everything written to the log from this thread while this is alive.
///
/// Deliberately not `Send`: a capture moved to another thread would read that
/// thread's sink, which is empty, and every assertion on it would fail in a
/// way that looks like the code under test went quiet.
pub(crate) struct Capture {
    _on_this_thread: PhantomData<*const ()>,
}

impl Capture {
    pub(crate) fn start() -> Self {
        CAPTURED.with(|sink| *sink.borrow_mut() = Some(Vec::new()));
        Self {
            _on_this_thread: PhantomData,
        }
    }

    /// The lines written so far, in the order they were written.
    pub(crate) fn lines(&self) -> Vec<String> {
        CAPTURED.with(|sink| sink.borrow().clone().unwrap_or_default())
    }

    /// Whether any captured line contains `needle`.
    pub(crate) fn saw(&self, needle: &str) -> bool {
        self.lines().iter().any(|line| line.contains(needle))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        CAPTURED.with(|sink| *sink.borrow_mut() = None);
    }
}

/// Keep `line` in this thread's capture. `false` means no capture is open
/// here and the line should be written for real.
pub(super) fn intercept(line: &str) -> bool {
    CAPTURED.with(|sink| match sink.borrow_mut().as_mut() {
        Some(lines) => {
            lines.push(line.to_string());
            true
        }
        None => false,
    })
}

/// Run an async test body with the log captured, and hand back what it
/// returned together with every line it wrote.
///
/// A plain `fn` driving its own current-thread runtime rather than a
/// `#[tokio::test]`: the body has to run on the thread that owns the capture,
/// and a current-thread runtime is what guarantees that.
pub(crate) fn captured<F>(body: impl FnOnce() -> F) -> (F::Output, Vec<String>)
where
    F: std::future::Future,
{
    let capture = Capture::start();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime for the test");
    let out = runtime.block_on(body());
    (out, capture.lines())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applog::{info, warn};

    /// The flake this prevents: a test asserting its command wrote nothing
    /// failed when another test's warning, on another thread, landed in the
    /// one process-wide buffer at the same moment.
    #[test]
    fn a_line_logged_on_another_thread_does_not_land_in_this_tests_capture() {
        let capture = Capture::start();
        info("mine");
        std::thread::spawn(|| warn("noise from a parallel test"))
            .join()
            .expect("the other thread finishes");
        assert_eq!(capture.lines().len(), 1, "{:?}", capture.lines());
        assert!(capture.saw("INFO mine"));
        assert!(!capture.saw("noise"), "{:?}", capture.lines());
    }

    #[test]
    fn two_captures_on_two_threads_each_see_only_their_own_lines() {
        let other = std::thread::spawn(|| {
            let capture = Capture::start();
            warn("theirs");
            capture.lines()
        });
        let capture = Capture::start();
        info("ours");
        let theirs = other.join().expect("the other thread finishes");
        assert!(
            theirs.iter().all(|line| line.contains("theirs")),
            "{theirs:?}"
        );
        assert_eq!(theirs.len(), 1);
        assert_eq!(capture.lines().len(), 1, "{:?}", capture.lines());
        assert!(capture.saw("INFO ours"));
    }

    #[test]
    fn dropping_the_capture_sends_later_lines_back_to_the_log() {
        let first = Capture::start();
        info("while open");
        drop(first);
        // With nothing open on this thread the line falls through to the
        // normal path, so a fresh capture started afterwards has not seen it.
        info("after close");
        let second = Capture::start();
        assert!(second.lines().is_empty(), "{:?}", second.lines());
    }
}
