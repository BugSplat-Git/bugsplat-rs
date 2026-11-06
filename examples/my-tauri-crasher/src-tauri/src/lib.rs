//! my-tauri-crasher: a Tauri app integrated with BugSplat. Each button in the window triggers
//! a different kind of failure: a native crash, a Rust panic, or a JavaScript error.

use std::hint::black_box;

/// The BugSplat database comes from `BUGSPLAT_DATABASE`, falling back to BugSplat's public
/// `fred` sample database. `BUGSPLAT_QUIET=1` uploads without showing the crash dialog.
pub fn bugsplat_options() -> bugsplat::Options {
    let database = std::env::var("BUGSPLAT_DATABASE").unwrap_or_else(|_| "fred".into());
    let quiet = std::env::var_os("BUGSPLAT_QUIET").is_some();
    bugsplat::Options::new(database, "my-tauri-crasher", env!("CARGO_PKG_VERSION"))
        .upload_policy(if quiet {
            bugsplat::UploadPolicy::Quiet
        } else {
            bugsplat::UploadPolicy::Dialog
        })
        .open_support_url(!quiet)
}

pub fn run() {
    tauri::Builder::default()
        // Reports uncaught JavaScript errors and unhandled rejections from every webview.
        .plugin(tauri_plugin_bugsplat::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![native_crash, rust_panic])
        .run(tauri::generate_context!())
        .expect("error while running my-tauri-crasher");
}

/// Writes through a null pointer: a segfault on macOS and Linux, an access violation on
/// Windows. BugSplatMonitor captures it from outside the process.
#[tauri::command]
fn native_crash() {
    crash::null_pointer_write();
}

/// Indexes past the end of a vector. The panic hook reports it with the message, location and
/// thread name before the panic continues.
#[tauri::command]
fn rust_panic() {
    crash::index_out_of_bounds();
}

mod crash {
    use super::black_box;

    #[inline(never)]
    pub fn null_pointer_write() {
        let p: *mut u32 = black_box(std::ptr::null_mut());
        // Deliberately undefined behaviour: this is a crash test.
        unsafe { p.write_volatile(0xdead) };
    }

    #[inline(never)]
    pub fn index_out_of_bounds() {
        let settings: Vec<&str> = black_box(Vec::new());
        println!("{}", settings[0]);
    }
}
