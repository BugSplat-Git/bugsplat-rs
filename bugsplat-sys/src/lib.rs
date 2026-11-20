//! Raw FFI bindings to bugsplat-native, BugSplat's native SDK.
//!
//! You almost certainly want the safe [`bugsplat`](https://docs.rs/bugsplat) crate instead.
//!
//! The SDK is built from source with CMake against BugSplat's pinned Crashpad prebuilt and linked
//! statically. `BugSplatMonitor` and `BugSplatReporter` (and on Windows `BugSplatWer.dll`) are built
//! alongside it and must ship with the application; [`BUILT_MONITOR_PATH`] and
//! [`BUILT_REPORTER_PATH`] point at the copies built for this compilation so development builds
//! (`cargo run`) work without packaging. `BugSplatWer.dll` has no such fallback: the SDK only looks
//! for it next to the executable, so the build script copies it into `target/<profile>`.
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case, clippy::all)]

mod bindings {
    include!("bindings.rs");
}
pub use bindings::*;

/// `BugSplatMonitor` built by this crate's build script.
pub const BUILT_MONITOR_PATH: Option<&str> = option_env!("BUGSPLAT_SYS_MONITOR");

/// `BugSplatReporter` (an `.app` bundle on macOS) built by this crate's build script.
pub const BUILT_REPORTER_PATH: Option<&str> = option_env!("BUGSPLAT_SYS_REPORTER");

/// `BugSplatWer.dll` built by this crate's build script (Windows only; `None` elsewhere).
pub const BUILT_WER_PATH: Option<&str> = option_env!("BUGSPLAT_SYS_WER");

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::CStr;

    #[test]
    fn links_and_reports_versions() {
        // Resolving these symbols proves the merged static archive and system libraries link.
        let version = unsafe { CStr::from_ptr(bugsplat_version_string()) };
        let header_version = CStr::from_bytes_with_nul(BUGSPLAT_SDK_VERSION).unwrap();
        assert_eq!(version, header_version);
        assert_eq!(unsafe { bugsplat_abi_version() }, BUGSPLAT_ABI_VERSION);
    }

    #[test]
    fn not_initialized_until_init() {
        assert_eq!(unsafe { bugsplat_is_initialized() }, 0);
        assert!(unsafe { bugsplat_get_environment() }.is_null());
        let result = unsafe { bugsplat_set_attribute(c"k".as_ptr(), c"v".as_ptr()) };
        assert_eq!(result, bugsplat_result::BUGSPLAT_ERR_NOT_INITIALIZED);
    }

    #[test]
    fn helpers_were_built() {
        for path in [BUILT_MONITOR_PATH, BUILT_REPORTER_PATH] {
            let path = path.expect("build script exports helper paths");
            assert!(std::path::Path::new(path).exists(), "{path} missing");
        }
    }

    #[test]
    #[cfg(windows)]
    fn wer_helper_was_built_and_copied_next_to_executables() {
        let path = BUILT_WER_PATH.expect("build script exports wer= on Windows");
        assert!(std::path::Path::new(path).exists(), "{path} missing");
        // Test binaries run from target/<profile>/deps, one of the directories build.rs copies to.
        let exe = std::env::current_exe().unwrap();
        let copy = exe.parent().unwrap().join("BugSplatWer.dll");
        assert!(copy.exists(), "{} missing", copy.display());
    }

    #[test]
    #[cfg(not(windows))]
    fn wer_helper_is_windows_only() {
        assert_eq!(BUILT_WER_PATH, None);
    }
}
