//! Triggers each kind of failure bugsplat-rs reports.
//!
//! ```sh
//! BUGSPLAT_DATABASE=my-db cargo run -p crasher -- <mode> [dialog]
//! ```
//!
//! Modes: segv, abort, stack-overflow, panic, panic-thread, panic-ffi, capture, js, none.
//! Uploads quietly unless `dialog` is passed; `manual` keeps the report in the local store.

use std::hint::black_box;

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "segv".into());
    let policy = match args.next().as_deref() {
        Some("dialog") => bugsplat::UploadPolicy::Dialog,
        Some("manual") => bugsplat::UploadPolicy::Manual,
        _ => bugsplat::UploadPolicy::Quiet,
    };
    let dialog = policy == bugsplat::UploadPolicy::Dialog;
    let database = std::env::var("BUGSPLAT_DATABASE").unwrap_or_else(|_| "fred".into());

    let options = bugsplat::Options::new(database, "Crasher", env!("CARGO_PKG_VERSION"))
        .upload_policy(policy)
        .open_support_url(dialog)
        .attribute("mode", mode.clone());
    let _guard = bugsplat::init(options).expect("bugsplat::init");
    println!("bugsplat-native {} | {}", bugsplat::native_version(), bugsplat::environment().unwrap_or_default());

    match mode.as_str() {
        "segv" => segv(),
        "abort" => std::process::abort(),
        "stack-overflow" => println!("{}", recurse(black_box(0))),
        "panic" => panic_in_main(),
        "panic-thread" => panic_in_thread(),
        "panic-ffi" => panic_through_ffi(),
        "capture" => {
            bugsplat::capture_report(Some("capture")).expect("capture_report");
            println!("captured; still running");
        }
        "js" => post_js(),
        "none" => println!("initialized; exiting normally"),
        other => eprintln!("unknown mode {other}"),
    }
}

#[inline(never)]
fn segv() {
    let p: *mut u32 = black_box(std::ptr::null_mut());
    unsafe { p.write_volatile(42) };
}

#[inline(never)]
#[allow(unconditional_recursion)]
fn recurse(n: u64) -> u64 {
    let pad = black_box([n; 512]);
    recurse(n + 1) + pad[0]
}

#[inline(never)]
fn panic_in_main() {
    let v: Vec<u32> = black_box(Vec::new());
    println!("{}", v[3]); // index out of bounds: unwinds out of main, exit status 101
}

#[inline(never)]
fn panic_in_thread() {
    let worker = std::thread::Builder::new()
        .name("worker".into())
        .spawn(|| {
            let parsed: Option<u32> = black_box("nope").parse().ok();
            parsed.expect("worker could not parse its input");
        })
        .unwrap();
    assert!(worker.join().is_err());
    println!("worker panicked; main thread still running");
}

/// A panic unwinding into an `extern "C"` frame: Rust aborts ("panic in a function that cannot
/// unwind"), which is what happens to panics in Tauri commands called from Objective-C.
#[inline(never)]
fn panic_through_ffi() {
    extern "C" fn callback() {
        panic!("panic inside an extern \"C\" callback");
    }
    let f: extern "C" fn() = black_box(callback);
    f();
}

fn post_js() {
    let report = bugsplat::js::JsReport {
        exceptions: serde_json::json!([{
            "id": 0, "parentId": null, "source": null,
            "type": "TypeError",
            "message": "Cannot read properties of undefined (reading 'title')",
            "stack": "renderDocument@tauri://localhost/js/app.js:42:17\nloadPending@tauri://localhost/js/app.js:91:9",
            "properties": {}
        }]),
        description: Some("crasher js mode".into()),
        ..Default::default()
    };
    match bugsplat::js::post(&report) {
        Ok(r) => println!("posted: {r:?}"),
        Err(e) => eprintln!("post failed: {e}"),
    }
}
