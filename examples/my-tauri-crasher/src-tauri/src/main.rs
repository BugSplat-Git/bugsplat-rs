// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Start BugSplat first, so a crash while Tauri starts up is reported too. Keep the guard
    // alive for the whole run: dropping it stops crash reporting.
    let _bugsplat =
        bugsplat::init(my_tauri_crasher_lib::bugsplat_options()).expect("BugSplat failed to start");

    my_tauri_crasher_lib::run();
}
