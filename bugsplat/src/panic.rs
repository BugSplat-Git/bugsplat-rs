//! Rust panics as BugSplat reports.
//!
//! A panic is not a crash as far as the OS is concerned: it either unwinds (and the process may
//! carry on, or exit with status 101 when the main thread unwinds out of `main`) or aborts. Neither
//! reaches Crashpad with useful context, so the hook does the work itself:
//!
//! 1. Records `rust.panic.message`, `rust.panic.location` and `rust.panic.thread` as attributes.
//! 2. Tags the next dump `panic` and calls `bugsplat_capture_report`, which writes a minidump of
//!    the live process out of process, synchronously, with the panicking thread on top.
//! 3. Clears the attributes so they cannot leak into a later, unrelated report.
//! 4. If the process is certain to abort next (`panic = "abort"`, or the "panic in a function
//!    that cannot unwind" re-panic Rust raises when an unwind hits an `extern "C"` boundary, as in
//!    Tauri commands invoked from Objective-C), tags the following dump `panic-duplicate`, which
//!    BugSplatMonitor discards, so one panic produces one report.

use std::panic::{self, PanicHookInfo};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

/// Panic reports per process; a panic storm should not flood the database.
const MAX_PANIC_REPORTS: usize = 8;
/// A nounwind re-panic within this window of a captured panic is the same failure.
const REPANIC_WINDOW: Duration = Duration::from_secs(5);
const MAX_MESSAGE_BYTES: usize = 4096;

pub const ATTR_MESSAGE: &str = "rust.panic.message";
pub const ATTR_LOCATION: &str = "rust.panic.location";
pub const ATTR_THREAD: &str = "rust.panic.thread";

static INSTALL: Once = Once::new();
static REPORTED: AtomicUsize = AtomicUsize::new(0);
static LAST_CAPTURE: Mutex<Option<Instant>> = Mutex::new(None);

/// Messages Rust uses when a panic cannot unwind; the process aborts right after the hook.
const NOUNWIND_MESSAGES: &[&str] = &[
    "panic in a function that cannot unwind",
    "panic in a destructor during cleanup",
];

pub(crate) fn install() {
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            report(info);
            previous(info);
        }));
    });
}

/// What the hook decided to do; separated from the SDK calls so it can be tested.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Capture a report, then mark the next dump as a duplicate if an abort is certain.
    Capture { abort_follows: bool },
    /// The abort that follows is a duplicate of an already captured panic.
    SuppressAbort,
    /// Over the per-process limit.
    Skip,
}

pub(crate) fn decide(
    message: &str,
    reported_so_far: usize,
    since_last_capture: Option<Duration>,
    panic_aborts: bool,
) -> Action {
    let nounwind = NOUNWIND_MESSAGES.iter().any(|m| message.starts_with(m));
    if nounwind && since_last_capture.is_some_and(|d| d <= REPANIC_WINDOW) {
        return Action::SuppressAbort;
    }
    if reported_so_far >= MAX_PANIC_REPORTS {
        return Action::Skip;
    }
    Action::Capture { abort_follows: panic_aborts || nounwind }
}

pub(crate) fn message<'a>(info: &'a PanicHookInfo<'_>) -> &'a str {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s
    } else {
        "Box<dyn Any>"
    }
}

pub(crate) fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn report(info: &PanicHookInfo<'_>) {
    if !crate::is_initialized() {
        return;
    }
    let message = message(info);
    // Serialize reports from concurrent panics; a poisoned lock still works.
    let mut last = LAST_CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    let action = decide(
        message,
        REPORTED.load(Ordering::Relaxed),
        last.map(|t| t.elapsed()),
        cfg!(panic = "abort"),
    );

    match action {
        Action::Skip => {}
        Action::SuppressAbort => {
            let _ = crate::set_next_report_kind(Some("panic-duplicate"));
        }
        Action::Capture { abort_follows } => {
            REPORTED.fetch_add(1, Ordering::Relaxed);
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_default();
            let current = std::thread::current();
            let thread = current.name().unwrap_or("<unnamed>");

            let _ = crate::set_attribute(ATTR_MESSAGE, Some(truncate(message, MAX_MESSAGE_BYTES)));
            let _ = crate::set_attribute(ATTR_LOCATION, Some(&location));
            let _ = crate::set_attribute(ATTR_THREAD, Some(thread));
            let _ = crate::capture_report(Some("panic"));
            for name in [ATTR_MESSAGE, ATTR_LOCATION, ATTR_THREAD] {
                let _ = crate::set_attribute(name, None);
            }
            *last = Some(Instant::now());
            if abort_follows {
                let _ = crate::set_next_report_kind(Some("panic-duplicate"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwinding_panic_is_captured_once() {
        assert_eq!(
            decide("index out of bounds", 0, None, false),
            Action::Capture { abort_follows: false }
        );
    }

    #[test]
    fn panic_abort_builds_suppress_the_following_abort() {
        assert_eq!(decide("boom", 0, None, true), Action::Capture { abort_follows: true });
    }

    #[test]
    fn nounwind_repanic_after_capture_is_a_duplicate() {
        let d = Some(Duration::from_millis(3));
        assert_eq!(
            decide("panic in a function that cannot unwind", 1, d, false),
            Action::SuppressAbort
        );
    }

    #[test]
    fn nounwind_panic_without_prior_capture_is_reported() {
        // e.g. a destructor panicking during cleanup with no earlier report.
        assert_eq!(
            decide("panic in a destructor during cleanup", 0, None, false),
            Action::Capture { abort_follows: true }
        );
        let stale = Some(Duration::from_secs(60));
        assert_eq!(
            decide("panic in a function that cannot unwind", 1, stale, false),
            Action::Capture { abort_follows: true }
        );
    }

    #[test]
    fn limit_stops_reports() {
        assert_eq!(decide("boom", MAX_PANIC_REPORTS, None, false), Action::Skip);
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("héllo", 2), "h");
        assert_eq!(truncate("héllo", 3), "hé");
        assert_eq!(truncate("abc", 10), "abc");
    }
}
