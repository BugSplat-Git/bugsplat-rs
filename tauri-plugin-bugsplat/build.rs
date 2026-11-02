const COMMANDS: &[&str] = &["report_error"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();

    // Re-export bugsplat-sys's helper paths so an app's build script can bundle them:
    // DEP_TAURI_PLUGIN_BUGSPLAT_MONITOR / DEP_TAURI_PLUGIN_BUGSPLAT_REPORTER.
    for key in ["MONITOR", "REPORTER"] {
        if let Ok(path) = std::env::var(format!("DEP_BUGSPLAT_{key}")) {
            println!("cargo:{}={path}", key.to_lowercase());
        }
    }
}
