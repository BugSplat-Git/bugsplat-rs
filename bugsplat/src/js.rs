//! JavaScript errors from embedded webviews (Tauri, wry, Dioxus), posted to BugSplat's
//! `post/js` endpoint with the same fields as [bugsplat-js](https://github.com/BugSplat-Git/bugsplat-js),
//! so the server processes them exactly like browser reports (including source maps).
//!
//! The webview sends the exception chain (`{ id, parentId, source, type, message, stack,
//! properties }` entries, as bugsplat-js builds them) to Rust, which adds the app's metadata and
//! the same `session` attribute native crash reports carry.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Metadata;

/// One JavaScript error report.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsReport {
    /// bugsplat-js's exception chain: entry 0 is the thrown error.
    pub exceptions: serde_json::Value,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    /// Files to attach (read by Rust, never by the webview).
    #[serde(skip)]
    pub attachments: Vec<PathBuf>,
}

/// The server's answer.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct JsResponse {
    pub status: Option<String>,
    #[serde(alias = "crash_id", alias = "crashId")]
    pub crash_id: Option<i64>,
    #[serde(alias = "infoUrl")]
    pub url: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum JsError {
    #[error("BugSplat is not initialized")]
    NotInitialized,
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("request failed: {0}")]
    Transport(String),
    #[error("attachment {0}: {1}")]
    Attachment(PathBuf, std::io::Error),
}

/// Overrides the endpoint (tests, proxies), like bugsplat-js's `BUGSPLAT_CRASH_POST_URL`.
pub const URL_ENV: &str = "BUGSPLAT_CRASH_POST_URL";

pub fn endpoint(database: &str) -> String {
    std::env::var(URL_ENV).unwrap_or_else(|_| format!("https://{database}.bugsplat.com/post/js/"))
}

/// Posts `report` with the metadata from [`crate::init`]. Blocks; call it off the UI thread.
pub fn post(report: &JsReport) -> Result<JsResponse, JsError> {
    let metadata = crate::metadata().ok_or(JsError::NotInitialized)?;
    post_with(&metadata, report, &endpoint(&metadata.database))
}

/// Posts `report` with explicit metadata to `url`.
pub fn post_with(metadata: &Metadata, report: &JsReport, url: &str) -> Result<JsResponse, JsError> {
    let form = build_form(metadata, report)?;
    let (content_type, body) = form.finish();

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(url)
        .header("Content-Type", &content_type)
        .send(&body[..])
        .map_err(|e| JsError::Transport(e.to_string()))?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| JsError::Transport(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(JsError::Http { status, body: text });
    }
    Ok(serde_json::from_str(&text).unwrap_or(JsResponse {
        status: Some("success".into()),
        crash_id: None,
        url: None,
        message: Some(text),
    }))
}

pub(crate) fn build_form(metadata: &Metadata, report: &JsReport) -> Result<Multipart, JsError> {
    let mut attributes = report.attributes.clone();
    attributes.insert("session".into(), metadata.session.clone());

    let mut form = Multipart::new();
    form.text("database", &metadata.database);
    form.text("appName", &metadata.application);
    form.text("appVersion", &metadata.version);
    form.text("appKey", metadata.key.as_deref().unwrap_or(""));
    form.text("user", metadata.user.as_deref().unwrap_or(""));
    form.text("email", metadata.email.as_deref().unwrap_or(""));
    form.text(
        "description",
        report.description.as_deref().or(metadata.description.as_deref()).unwrap_or(""),
    );
    let exceptions = serde_json::json!({ "exceptions": report.exceptions });
    form.text("exceptions", &exceptions.to_string());
    form.text("attributes", &serde_json::to_string(&attributes).unwrap_or_default());
    for path in &report.attachments {
        let data = std::fs::read(path).map_err(|e| JsError::Attachment(path.clone(), e))?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        form.file(&name, &name, &data);
    }
    Ok(form)
}

/// Minimal multipart/form-data encoder (text fields and files).
pub(crate) struct Multipart {
    boundary: String,
    body: Vec<u8>,
}

impl Multipart {
    pub(crate) fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        Multipart { boundary: format!("----bugsplat-rs-{nanos:x}"), body: Vec::new() }
    }

    fn header(&mut self, disposition: &str) {
        self.body.extend_from_slice(format!("--{}\r\n{disposition}\r\n", self.boundary).as_bytes());
    }

    pub(crate) fn text(&mut self, name: &str, value: &str) {
        self.header(&format!("Content-Disposition: form-data; name=\"{}\"\r\n", escape(name)));
        self.body.extend_from_slice(value.as_bytes());
        self.body.extend_from_slice(b"\r\n");
    }

    pub(crate) fn file(&mut self, name: &str, filename: &str, data: &[u8]) {
        self.header(&format!(
            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n",
            escape(name),
            escape(filename)
        ));
        self.body.extend_from_slice(data);
        self.body.extend_from_slice(b"\r\n");
    }

    pub(crate) fn finish(mut self) -> (String, Vec<u8>) {
        self.body.extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        (format!("multipart/form-data; boundary={}", self.boundary), self.body)
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> Metadata {
        Metadata {
            database: "db".into(),
            application: "App".into(),
            version: "1.2.3".into(),
            user: Some("fred".into()),
            session: "42-1700000000".into(),
            ..Default::default()
        }
    }

    fn fields(body: &[u8]) -> String {
        String::from_utf8_lossy(body).into_owned()
    }

    #[test]
    fn form_carries_bugsplat_js_fields_and_session() {
        let report = JsReport {
            exceptions: serde_json::json!([{ "id": 0, "type": "TypeError", "message": "x is undefined" }]),
            attributes: [("route".to_string(), "/home".to_string())].into(),
            ..Default::default()
        };
        let (content_type, body) = build_form(&metadata(), &report).unwrap().finish();
        let body = fields(&body);
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        for field in ["database", "appName", "appVersion", "appKey", "user", "email", "description", "exceptions", "attributes"] {
            assert!(body.contains(&format!("name=\"{field}\"")), "missing {field}");
        }
        assert!(body.contains("\r\n\r\nApp\r\n"));
        assert!(body.contains(r#"{"exceptions":[{"id":0,"message":"x is undefined","type":"TypeError"}]}"#));
        assert!(body.contains(r#"{"route":"/home","session":"42-1700000000"}"#));
        assert!(body.trim_end().ends_with("--"));
    }

    #[test]
    fn report_description_overrides_default() {
        let mut m = metadata();
        m.description = Some("default".into());
        let report = JsReport { description: Some("specific".into()), ..Default::default() };
        let (_, body) = build_form(&m, &report).unwrap().finish();
        assert!(fields(&body).contains("\r\n\r\nspecific\r\n"));
    }

    #[test]
    fn attachments_become_file_parts() {
        let dir = std::env::temp_dir().join(format!("bugsplat-js-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("app.log");
        std::fs::write(&file, "log line").unwrap();
        let report = JsReport { attachments: vec![file], ..Default::default() };
        let (_, body) = build_form(&metadata(), &report).unwrap().finish();
        let body = fields(&body);
        assert!(body.contains("name=\"app.log\"; filename=\"app.log\""));
        assert!(body.contains("log line"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_attachment_is_an_error() {
        let report = JsReport { attachments: vec!["/nonexistent/file.log".into()], ..Default::default() };
        assert!(matches!(build_form(&metadata(), &report), Err(JsError::Attachment(..))));
    }

    #[test]
    fn header_values_are_escaped() {
        assert_eq!(escape("a\"b\r\nc"), "a\\\"b  c");
    }

    #[test]
    fn response_accepts_server_field_spellings() {
        let r: JsResponse = serde_json::from_str(r#"{"status":"success","crash_id":7,"url":"u"}"#).unwrap();
        assert_eq!(r.crash_id, Some(7));
        let r: JsResponse = serde_json::from_str(r#"{"status":"success","crashId":8,"infoUrl":"u"}"#).unwrap();
        assert_eq!(r.crash_id, Some(8));
    }
}
