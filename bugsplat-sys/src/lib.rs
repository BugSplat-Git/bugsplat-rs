//! Raw FFI bindings to [bugsplat-native](https://github.com/BugSplat-Git/bugsplat-native).
//!
//! You almost certainly want the safe [`bugsplat`](https://docs.rs/bugsplat) crate instead.
//!
//! The SDK is built from source with CMake against BugSplat's pinned Crashpad prebuilt and linked
//! statically. `BugSplatMonitor` and `BugSplatReporter` are built alongside it and must ship with
//! the application; [`BUILT_MONITOR_PATH`] and [`BUILT_REPORTER_PATH`] point at the copies built
//! for this compilation so development builds (`cargo run`) work without packaging.
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case, clippy::all)]

mod bindings {
    include!("bindings.rs");
}
pub use bindings::*;

/// `BugSplatMonitor` built by this crate's build script.
pub const BUILT_MONITOR_PATH: Option<&str> = option_env!("BUGSPLAT_SYS_MONITOR");

/// `BugSplatReporter` (an `.app` bundle on macOS) built by this crate's build script.
pub const BUILT_REPORTER_PATH: Option<&str> = option_env!("BUGSPLAT_SYS_REPORTER");

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
}
