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

    /// The Subsystem field of a PE image's optional header. It sits 68 bytes in for both PE32
    /// and PE32+: the optional header starts after the "PE\0\0" signature and the 20-byte COFF
    /// header, whose offset is the u32 at 0x3C.
    fn pe_subsystem(image: &[u8]) -> u16 {
        let u16_at = |at: usize| u16::from_le_bytes([image[at], image[at + 1]]);
        assert_eq!(&image[..2], b"MZ", "not a PE image");
        let pe = u32::from_le_bytes(image[0x3C..0x40].try_into().unwrap()) as usize;
        assert_eq!(&image[pe..pe + 4], b"PE\0\0", "not a PE image");
        u16_at(pe + 4 + 20 + 68)
    }

    #[test]
    fn pe_subsystem_reads_the_optional_header() {
        // A minimal image: the DOS header points at a PE header at 0x80.
        let mut image = vec![0u8; 0x80 + 4 + 20 + 70];
        image[..2].copy_from_slice(b"MZ");
        image[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        image[0x80..0x84].copy_from_slice(b"PE\0\0");
        image[0x80 + 4 + 20 + 68] = 2;
        assert_eq!(pe_subsystem(&image), 2);
    }

    /// Crashpad starts BugSplatMonitor.exe without CREATE_NO_WINDOW, so as a console program it
    /// opened a console window for the whole life of every GUI app (issue #23). It must be a
    /// windowed program (IMAGE_SUBSYSTEM_WINDOWS_GUI).
    #[test]
    #[cfg(windows)]
    fn monitor_is_a_windowed_program() {
        const IMAGE_SUBSYSTEM_WINDOWS_GUI: u16 = 2;
        let path = BUILT_MONITOR_PATH.expect("build script exports monitor=");
        let image = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(pe_subsystem(&image), IMAGE_SUBSYSTEM_WINDOWS_GUI, "{path} is not a windowed program");
    }

    #[test]
    #[cfg(windows)]
    fn wer_helper_was_built_and_copied_next_to_executables() {
        let path = BUILT_WER_PATH.expect("build script exports wer= on Windows");
        assert!(std::path::Path::new(path).exists(), "{path} missing");
        // build.rs copies it into the profile directory (target/<profile>, marked by Cargo's
        // .cargo-lock), where `cargo run` starts executables. Test binaries live below it.
        let exe = std::env::current_exe().unwrap();
        let profile_dir = exe
            .ancestors()
            .find(|dir| dir.join(".cargo-lock").exists())
            .expect("test binary runs inside a Cargo profile directory");
        let copy = profile_dir.join("BugSplatWer.dll");
        assert!(copy.exists(), "{} missing", copy.display());
    }

    #[test]
    #[cfg(not(windows))]
    fn wer_helper_is_windows_only() {
        assert_eq!(BUILT_WER_PATH, None);
    }
}
