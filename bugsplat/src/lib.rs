//! Crash reporting for Rust applications with [BugSplat](https://www.bugsplat.com).
//!
//! Built on [bugsplat-native](https://github.com/BugSplat-Git/bugsplat-native): crashes (segfaults,
//! aborts, stack overflows) are captured out of process by Crashpad via `BugSplatMonitor`, shown to
//! the user by `BugSplatReporter` and uploaded as minidumps. Rust panics are reported too (see
//! [`panic`]). Upload symbols with `symbol-upload -m` so the server can symbolicate.
//!
//! ```no_run
//! fn main() {
//!     let _bugsplat = bugsplat::init(
//!         bugsplat::Options::new("my-database", "MyApp", env!("CARGO_PKG_VERSION")),
//!     )
//!     .expect("BugSplat failed to start");
//!     // ... the rest of the application; keep the guard alive until exit.
//! }
//! ```

// The examples keep `fn main` to show that the guard lives for the whole program.
#![allow(clippy::needless_doctest_main)]

mod error;
mod options;
pub mod panic;

#[cfg(feature = "js")]
pub mod js;

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use bugsplat_sys as sys;

pub use error::Error;
pub use options::{DumpType, HangPolicy, Options, UploadPolicy};

/// Report metadata mirrored on the Rust side, for reports Rust sends itself (JavaScript errors).
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub database: String,
    pub application: String,
    pub version: String,
    pub key: Option<String>,
    pub user: Option<String>,
    pub email: Option<String>,
    pub description: Option<String>,
    /// Identifies this process run; set as the `session` attribute on every report.
    pub session: String,
}

static METADATA: Mutex<Option<Metadata>> = Mutex::new(None);

fn metadata_lock() -> MutexGuard<'static, Option<Metadata>> {
    METADATA.lock().unwrap_or_else(|e| e.into_inner())
}

/// A snapshot of the current report metadata, or `None` before [`init`].
pub fn metadata() -> Option<Metadata> {
    metadata_lock().clone()
}

/// Keeps BugSplat running; dropping it calls `bugsplat_shutdown`.
#[must_use = "BugSplat shuts down when the guard is dropped"]
#[derive(Debug)]
pub struct Guard {
    _private: (),
}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe { sys::bugsplat_shutdown() };
        *metadata_lock() = None;
    }
}

/// Starts crash reporting. Call once, as early as possible in `main`.
pub fn init(options: Options) -> Result<Guard, Error> {
    let session = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default()
    );
    let options = options.attribute("session", session.clone());

    // Unpackaged development builds use the helpers bugsplat-sys built. Checking first (rather than
    // retrying after BUGSPLAT_ERR_MONITOR_NOT_FOUND) keeps a spurious error out of BugSplat.log.
    let explicit_helpers = options.monitor_path.is_some() || options.reporter_path.is_some();
    let dev = (options.development_helpers && !explicit_helpers && !helpers_packaged())
        .then(development_helpers)
        .flatten();
    init_native(&options, dev.as_ref().map(|(m, r)| (m.as_path(), r.as_path())))?;

    *metadata_lock() = Some(Metadata {
        database: options.database.clone(),
        application: options.application.clone(),
        version: options.version.clone(),
        key: options.key.clone(),
        user: options.user.clone(),
        email: options.email.clone(),
        description: options.description.clone(),
        session,
    });
    if options.panic_reports {
        panic::install();
    }
    Ok(Guard { _private: () })
}

fn init_native(options: &Options, helpers: Option<(&Path, &Path)>) -> Result<(), Error> {
    let native = options.to_native(helpers)?;
    // bugsplat_init takes ownership of the options object, even on failure.
    Error::check(unsafe { sys::bugsplat_init(native) })
}

/// Whether `BugSplatMonitor` ships with the app in one of the places bugsplat-native searches.
fn helpers_packaged() -> bool {
    let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else {
        return false;
    };
    helper_candidates(&exe_dir).iter().any(|p| p.exists())
}

pub(crate) fn helper_candidates(exe_dir: &Path) -> Vec<PathBuf> {
    let monitor = if cfg!(windows) { "BugSplatMonitor.exe" } else { "BugSplatMonitor" };
    let mut candidates = vec![exe_dir.join(monitor), exe_dir.join("Helpers").join(monitor)];
    if cfg!(target_os = "macos") {
        // <App>.app/Contents/MacOS/<exe> -> Contents/Helpers, Contents/Frameworks/*.framework/Helpers
        let contents = exe_dir.join("..");
        candidates.push(contents.join("Helpers").join(monitor));
        for framework in ["BugSplatNative.framework", "BugSplat.framework"] {
            candidates.push(contents.join("Frameworks").join(framework).join("Helpers").join(monitor));
        }
    }
    candidates
}

/// The helpers `bugsplat-sys` built for this compilation, if they still exist on disk.
fn development_helpers() -> Option<(PathBuf, PathBuf)> {
    let monitor = PathBuf::from(sys::BUILT_MONITOR_PATH?);
    let reporter = PathBuf::from(sys::BUILT_REPORTER_PATH?);
    (monitor.exists() && reporter.exists()).then_some((monitor, reporter))
}

pub fn is_initialized() -> bool {
    unsafe { sys::bugsplat_is_initialized() != 0 }
}

fn with_cstr(value: Option<&str>, f: impl FnOnce(*const std::ffi::c_char) -> sys::bugsplat_result) -> Result<(), Error> {
    let owned = value.map(CString::new).transpose()?;
    Error::check(f(owned.as_ref().map_or(std::ptr::null(), |c| c.as_ptr())))
}

fn update_metadata(f: impl FnOnce(&mut Metadata)) {
    if let Some(m) = metadata_lock().as_mut() {
        f(m);
    }
}

pub fn set_user(user: &str) -> Result<(), Error> {
    with_cstr(Some(user), |p| unsafe { sys::bugsplat_set_user(p) })?;
    update_metadata(|m| m.user = Some(user.to_string()));
    Ok(())
}

pub fn set_email(email: &str) -> Result<(), Error> {
    with_cstr(Some(email), |p| unsafe { sys::bugsplat_set_email(p) })?;
    update_metadata(|m| m.email = Some(email.to_string()));
    Ok(())
}

pub fn set_key(key: &str) -> Result<(), Error> {
    with_cstr(Some(key), |p| unsafe { sys::bugsplat_set_key(p) })?;
    update_metadata(|m| m.key = Some(key.to_string()));
    Ok(())
}

pub fn set_description(description: &str) -> Result<(), Error> {
    with_cstr(Some(description), |p| unsafe { sys::bugsplat_set_user_description(p) })?;
    update_metadata(|m| m.description = Some(description.to_string()));
    Ok(())
}

pub fn set_notes(notes: &str) -> Result<(), Error> {
    with_cstr(Some(notes), |p| unsafe { sys::bugsplat_set_notes(p) })
}

/// Overrides the detected environment; `None` restores detection.
pub fn set_environment(environment: Option<&str>) -> Result<(), Error> {
    with_cstr(environment, |p| unsafe { sys::bugsplat_set_environment(p) })
}

/// The OS/hardware description reports carry, e.g. `macOS 26.6.2 (25G83) arm64; Mac14,9`.
pub fn environment() -> Option<String> {
    let p = unsafe { sys::bugsplat_get_environment() };
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

/// Sets (or with `None`, removes) a custom attribute captured with every later report.
pub fn set_attribute(name: &str, value: Option<&str>) -> Result<(), Error> {
    let name = CString::new(name)?;
    with_cstr(value, |p| unsafe { sys::bugsplat_set_attribute(name.as_ptr(), p) })
}

/// Adds a file the monitor copies into every later report.
pub fn add_attachment(path: impl AsRef<Path>) -> Result<(), Error> {
    let path = options::path_cstring(path.as_ref())?;
    Error::check(unsafe { sys::bugsplat_add_attachment(path.as_ptr()) })
}

pub fn remove_attachment(path: impl AsRef<Path>) -> Result<(), Error> {
    let path = options::path_cstring(path.as_ref())?;
    Error::check(unsafe { sys::bugsplat_remove_attachment(path.as_ptr()) })
}

/// Switches between the crash dialog and silent uploads at runtime.
pub fn set_quiet_mode(quiet: bool) {
    unsafe { sys::bugsplat_set_quiet_mode(quiet as i32) }
}

/// Tags the next dump with a report kind (`None` clears it). Kinds ending in `-duplicate` are
/// discarded by the monitor.
pub fn set_next_report_kind(kind: Option<&str>) -> Result<(), Error> {
    with_cstr(kind, |p| unsafe { sys::bugsplat_set_next_report_kind(p) })
}

/// Writes a minidump of the running process and reports it without crashing.
pub fn capture_report(kind: Option<&str>) -> Result<(), Error> {
    if kind.is_some() {
        set_next_report_kind(kind)?;
    }
    Error::check(unsafe { sys::bugsplat_capture_report() })
}

/// For hang detection in apps without a platform main loop: call at least once per timeout.
pub fn heartbeat() {
    unsafe { sys::bugsplat_heartbeat() }
}

/// Watches the calling thread for hangs in addition to the main thread.
pub fn watch_current_thread(name: &str) -> Result<(), Error> {
    with_cstr(Some(name), |p| unsafe { sys::bugsplat_watch_thread(p) })
}

pub fn unwatch_current_thread() {
    unsafe { sys::bugsplat_unwatch_thread() }
}

/// The bugsplat-native version linked into this binary.
pub fn native_version() -> String {
    unsafe { CStr::from_ptr(sys::bugsplat_version_string()) }
        .to_string_lossy()
        .into_owned()
}

/// Path of `BugSplat.log` in the report store, after [`init`].
pub fn log_file_path() -> Option<PathBuf> {
    let p = unsafe { sys::bugsplat_log_file_path() };
    (!p.is_null()).then(|| PathBuf::from(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_before_init_report_not_initialized() {
        assert!(!is_initialized());
        assert_eq!(set_attribute("a", Some("b")), Err(Error::NotInitialized));
        assert_eq!(environment(), None);
        assert!(metadata().is_none());
    }

    #[test]
    fn interior_nul_is_rejected_without_calling_the_sdk() {
        assert_eq!(set_attribute("a\0b", Some("c")), Err(Error::Nul));
        assert_eq!(set_user("x\0"), Err(Error::Nul));
    }

    #[test]
    fn native_version_is_reported() {
        assert!(native_version().contains('.'));
    }

    #[test]
    fn development_helpers_exist_in_this_build() {
        assert!(development_helpers().is_some());
    }
}

#[cfg(test)]
mod packaging_tests {
    use super::*;

    #[test]
    fn looks_next_to_the_exe_and_in_bundles() {
        let candidates = helper_candidates(Path::new("/Apps/My.app/Contents/MacOS"));
        let names: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
        assert!(names[0].ends_with("MacOS/BugSplatMonitor") || names[0].ends_with("MacOS\\BugSplatMonitor.exe"));
        if cfg!(target_os = "macos") {
            assert!(names.iter().any(|n| n.ends_with("Contents/MacOS/../Helpers/BugSplatMonitor")));
            assert!(names.iter().any(|n| n.contains("Frameworks/BugSplatNative.framework/Helpers")));
        }
    }

    #[test]
    fn unpackaged_test_binary_has_no_helpers() {
        assert!(!helpers_packaged());
    }
}
