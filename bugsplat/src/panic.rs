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
//! 4. Tags the following dump `panic-duplicate`, which BugSplatMonitor discards, so one panic
//!    produces one report. When the process is certain to abort next (`panic = "abort"`, or the
//!    "panic in a function that cannot unwind" re-panic Rust raises when an unwind hits an
//!    `extern "C"` boundary), the tag stays. Otherwise it is provisional: the unwind can still
//!    fail outright with no re-panic ("failed to initiate panic", as in a Tauri command invoked
//!    from Objective-C on macOS) and abort straight away, so the tag covers that abort and is
//!    cleared `PROVISIONAL_WINDOW` after the hook chain returns if the process carries on instead.
//!    An explicit [`crate::capture_report`] is always filed as `capture`, and the SDK clears the
//!    next dump's kind after it, so it puts back any duplicate tag still pending.

use std::panic::{self, PanicHookInfo};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

/// Panic reports per process; a panic storm should not flood the database.
const MAX_PANIC_REPORTS: usize = 8;
/// A nounwind re-panic within this window of a captured panic is the same failure.
const REPANIC_WINDOW: Duration = Duration::from_secs(5);
/// How long a provisional `panic-duplicate` tag outlives the hook chain. An unwind that cannot
/// start aborts as soon as the hooks return, so this only needs to cover scheduling delays; any
/// unrelated crash inside it is discarded, so keep it short.
const PROVISIONAL_WINDOW: Duration = Duration::from_secs(1);
const MAX_MESSAGE_BYTES: usize = 4096;
const DUPLICATE_KIND: &str = "panic-duplicate";

pub const ATTR_MESSAGE: &str = "rust.panic.message";
pub const ATTR_LOCATION: &str = "rust.panic.location";
pub const ATTR_THREAD: &str = "rust.panic.thread";

static INSTALL: Once = Once::new();
static REPORTED: AtomicUsize = AtomicUsize::new(0);
/// Serializes the hook, provisional-tag expiry and explicit captures, which all set the kind of
/// the next dump.
static STATE: Mutex<State> = Mutex::new(State::new());

fn state() -> MutexGuard<'static, State> {
    // A poisoned lock still works.
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) struct State {
    last_capture: Option<Instant>,
    generation: u64,
    /// The `panic-duplicate` tag on the next dump, if there is one.
    tag: Option<Tag>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Tag {
    generation: u64,
    provisional: bool,
}

impl State {
    pub(crate) const fn new() -> Self {
        Self { last_capture: None, generation: 0, tag: None }
    }

    /// Records that the next dump is tagged a duplicate; returns the tag's generation.
    pub(crate) fn tag_duplicate(&mut self, provisional: bool) -> u64 {
        self.generation += 1;
        self.tag = Some(Tag { generation: self.generation, provisional });
        self.generation
    }

    /// Ends the provisional tag `generation`. True when it was still on the next dump and must be
    /// cleared; false when something has tagged the next dump since.
    pub(crate) fn expire(&mut self, generation: u64) -> bool {
        if self.tag == Some(Tag { generation, provisional: true }) {
            self.tag = None;
            true
        } else {
            false
        }
    }

    /// Whether the next dump is tagged a duplicate, so an explicit capture must put the tag back.
    pub(crate) fn tagged(&self) -> bool {
        self.tag.is_some()
    }
}

/// Clears a provisional tag after `PROVISIONAL_WINDOW`, under the lock the hook tags with, so it
/// can never clear a newer panic's tag.
fn expire_after_window(generation: u64) {
    // If the thread cannot be spawned the tag simply stays, which at worst drops one later dump.
    let _ = std::thread::Builder::new()
        .name("bugsplat-panic-duplicate".into())
        .spawn(move || {
            std::thread::sleep(PROVISIONAL_WINDOW);
            let mut state = state();
            if state.expire(generation) {
                let _ = crate::set_next_report_kind(None);
            }
        });
}

/// [`crate::capture_report`]: the SDK files the capture as `capture` and then clears the next
/// dump's kind, which would let the abort a panic tag covers be reported twice. Put the tag back.
pub(crate) fn capture_explicit(kind: Option<&str>) -> Result<(), crate::Error> {
    let state = state();
    let result = crate::capture_now(kind);
    if state.tagged() {
        let _ = crate::set_next_report_kind(Some(DUPLICATE_KIND));
    }
    result
}

/// Messages Rust uses when a panic cannot unwind; the process aborts right after the hook.
const NOUNWIND_MESSAGES: &[&str] = &[
    "panic in a function that cannot unwind",
    "panic in a destructor during cleanup",
];

pub(crate) fn install() {
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let provisional = report(info);
            previous(info);
            // Timed from here: the default hook can spend a while printing a backtrace.
            if let Some(generation) = provisional {
                expire_after_window(generation);
            }
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

/// Returns the generation of a provisional duplicate tag, which the caller expires.
fn report(info: &PanicHookInfo<'_>) -> Option<u64> {
    if !crate::is_initialized() {
        return None;
    }
    let message = message(info);
    // Serialize reports from concurrent panics.
    let mut state = state();
    let action = decide(
        message,
        REPORTED.load(Ordering::Relaxed),
        state.last_capture.map(|t| t.elapsed()),
        cfg!(panic = "abort"),
    );

    match action {
        Action::Skip => None,
        Action::SuppressAbort => {
            state.tag_duplicate(false);
            let _ = crate::set_next_report_kind(Some(DUPLICATE_KIND));
            None
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
            let _ = crate::capture_now(Some("panic"));
            for name in [ATTR_MESSAGE, ATTR_LOCATION, ATTR_THREAD] {
                let _ = crate::set_attribute(name, None);
            }
            state.last_capture = Some(Instant::now());
            let generation = state.tag_duplicate(!abort_follows);
            let _ = crate::set_next_report_kind(Some(DUPLICATE_KIND));
            (!abort_follows).then_some(generation)
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
    fn provisional_tag_expires_only_while_it_owns_the_tag() {
        let mut state = State::new();
        let first = state.tag_duplicate(true);

        // A later panic re-tagged the next dump: the first tag's expiry must leave it alone.
        let second = state.tag_duplicate(true);
        assert!(!state.expire(first));
        assert!(state.expire(second));
        // Expired once; a second expiry has nothing to clear.
        assert!(!state.expire(second));
    }

    #[test]
    fn certain_abort_tag_never_expires() {
        let mut state = State::new();
        let provisional = state.tag_duplicate(true);
        // A nounwind re-panic makes the abort certain: the earlier provisional expiry must not
        // clear the tag that covers it.
        let certain = state.tag_duplicate(false);
        assert!(!state.expire(provisional));
        assert!(!state.expire(certain));
        assert!(state.tagged());
    }

    #[test]
    fn explicit_captures_restore_a_pending_tag_until_it_expires() {
        let mut state = State::new();
        assert!(!state.tagged());
        let provisional = state.tag_duplicate(true);
        assert!(state.tagged());
        assert!(state.expire(provisional));
        assert!(!state.tagged());
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
