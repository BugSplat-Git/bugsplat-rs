//! [BugSplat](https://www.bugsplat.com) crash reporting for [Tauri](https://tauri.app).
//!
//! - **Native crashes** (segfaults, aborts, stack overflows) and **Rust panics** are reported by the
//!   [`bugsplat`] crate, which must be initialized at the top of `main`, before Tauri starts.
//! - **JavaScript errors** in any webview (uncaught errors and unhandled rejections) are reported
//!   by this plugin through Rust to BugSplat's `post/js` endpoint. Each report carries the same
//!   `session` attribute as native reports from that run, plus the Tauri and WebKit versions.
//!
//! ```ignore
//! fn main() {
//!     let _bugsplat = bugsplat::init(bugsplat::Options::new("my-db", "MyApp", env!("CARGO_PKG_VERSION")))
//!         .expect("BugSplat failed to start");
//!     tauri::Builder::default()
//!         .plugin(tauri_plugin_bugsplat::init())
//!         .run(tauri::generate_context!())
//!         .expect("error while running tauri application");
//! }
//! ```
//!
//! Add `"bugsplat:default"` to the app's capability, and ship `BugSplatMonitor` and
//! `BugSplatReporter` inside the bundle (see the README).

#![allow(clippy::needless_doctest_main)]

use std::collections::BTreeMap;

use bugsplat::js::JsReport;
use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Runtime, Webview};

const INIT_SCRIPT: &str = include_str!("../js/init.js");

/// What the webview sends: the exception chain and attributes from `js/init.js`.
#[derive(Debug, Deserialize)]
struct WebviewReport {
    exceptions: serde_json::Value,
    #[serde(default)]
    attributes: BTreeMap<String, String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportResult {
    crash_id: Option<i64>,
}

/// Attributes describing the Tauri runtime, added to native and JavaScript reports.
fn runtime_attributes() -> Vec<(&'static str, String)> {
    #[allow(unused_mut)]
    let mut attrs = vec![("tauri.version", tauri::VERSION.to_string())];
    #[cfg(feature = "wry")]
    if let Ok(webview) = tauri::webview_version() {
        attrs.push(("webview.version", webview));
    }
    attrs
}

#[tauri::command]
async fn report_error<R: Runtime>(
    webview: Webview<R>,
    report: WebviewReport,
) -> Result<ReportResult, String> {
    let mut attributes = report.attributes;
    attributes.insert("webview.label".into(), webview.label().to_string());
    for (k, v) in runtime_attributes() {
        attributes.insert(k.into(), v);
    }
    let report = JsReport {
        exceptions: report.exceptions,
        description: report.description,
        attributes,
        attachments: Vec::new(),
    };
    tauri::async_runtime::spawn_blocking(move || bugsplat::js::post(&report))
        .await
        .map_err(|e| e.to_string())?
        .map(|r| ReportResult { crash_id: r.crash_id })
        .map_err(|e| e.to_string())
}

/// The plugin. Call [`bugsplat::init`] first; without it JavaScript reports are dropped.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("bugsplat")
        .js_init_script(INIT_SCRIPT.to_string())
        .invoke_handler(tauri::generate_handler![report_error])
        .setup(|_app, _api| {
            if bugsplat::is_initialized() {
                for (k, v) in runtime_attributes() {
                    let _ = bugsplat::set_attribute(k, Some(&v));
                }
            } else {
                eprintln!("tauri-plugin-bugsplat: call bugsplat::init() in main before starting Tauri");
            }
            Ok(())
        })
        .build()
}
