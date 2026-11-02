use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bugsplat_sys as sys;

use crate::Error;

/// What happens to a report once it has been captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UploadPolicy {
    /// Show the BugSplat dialog, then upload.
    #[default]
    Dialog,
    /// Upload without any UI.
    Quiet,
    /// Keep reports pending until the app sends them.
    Manual,
}

/// How much process memory a dump includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DumpType {
    /// Threads, stacks, modules and the exception (Crashpad's default).
    #[default]
    Normal,
    /// Plus private writable memory.
    Heap,
    /// Every readable region.
    Full,
}

/// What to do when the hang watchdog fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HangPolicy {
    /// Dump the live process, report it and keep running.
    Report,
    /// Report, then offer Wait / Close and terminate on Close.
    ReportAndTerminate,
}

/// Configuration for [`crate::init`].
#[derive(Debug, Clone)]
pub struct Options {
    pub(crate) database: String,
    pub(crate) application: String,
    pub(crate) version: String,
    pub(crate) upload_policy: UploadPolicy,
    pub(crate) dump_type: DumpType,
    pub(crate) hang: Option<(Duration, HangPolicy)>,
    pub(crate) store_dir: Option<PathBuf>,
    pub(crate) monitor_path: Option<PathBuf>,
    pub(crate) reporter_path: Option<PathBuf>,
    pub(crate) open_support_url: Option<bool>,
    pub(crate) key: Option<String>,
    pub(crate) user: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) notes: Option<String>,
    pub(crate) environment: Option<String>,
    pub(crate) attributes: Vec<(String, String)>,
    pub(crate) attachments: Vec<PathBuf>,
    pub(crate) panic_reports: bool,
    pub(crate) development_helpers: bool,
}

impl Options {
    /// `database`, `application` and `version` must match the values used to upload symbols.
    pub fn new(
        database: impl Into<String>,
        application: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Options {
            database: database.into(),
            application: application.into(),
            version: version.into(),
            upload_policy: UploadPolicy::default(),
            dump_type: DumpType::default(),
            hang: None,
            store_dir: None,
            monitor_path: None,
            reporter_path: None,
            open_support_url: None,
            key: None,
            user: None,
            email: None,
            description: None,
            notes: None,
            environment: None,
            attributes: Vec::new(),
            attachments: Vec::new(),
            panic_reports: true,
            development_helpers: true,
        }
    }

    pub fn upload_policy(mut self, policy: UploadPolicy) -> Self {
        self.upload_policy = policy;
        self
    }

    pub fn dump_type(mut self, dump_type: DumpType) -> Self {
        self.dump_type = dump_type;
        self
    }

    /// Report the main thread as hung after `timeout` without progress.
    pub fn hang_detection(mut self, timeout: Duration, policy: HangPolicy) -> Self {
        self.hang = Some((timeout, policy));
        self
    }

    /// Where reports are stored before upload (defaults to the per-user app data directory).
    pub fn store_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.store_dir = Some(dir.into());
        self
    }

    /// Explicit `BugSplatMonitor` location (by default the SDK looks next to the app).
    pub fn monitor_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.monitor_path = Some(path.into());
        self
    }

    /// Explicit `BugSplatReporter` location (by default the SDK looks next to the app).
    pub fn reporter_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.reporter_path = Some(path.into());
        self
    }

    /// Open the support-response URL after an interactive upload (default on for desktop).
    pub fn open_support_url(mut self, open: bool) -> Self {
        self.open_support_url = Some(open);
        self
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
    }

    /// Overrides the auto-detected OS/hardware description.
    pub fn environment(mut self, environment: impl Into<String>) -> Self {
        self.environment = Some(environment.into());
        self
    }

    pub fn attribute(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.push((name.into(), value.into()));
        self
    }

    pub fn attachment(mut self, path: impl Into<PathBuf>) -> Self {
        self.attachments.push(path.into());
        self
    }

    /// Report Rust panics (default on). See [`crate::panic`] for exactly what is captured.
    pub fn panic_reports(mut self, enabled: bool) -> Self {
        self.panic_reports = enabled;
        self
    }

    /// When the helpers are not packaged next to the app, fall back to the copies `bugsplat-sys`
    /// built for this compilation (default on), so `cargo run` works without packaging.
    pub fn development_helpers(mut self, enabled: bool) -> Self {
        self.development_helpers = enabled;
        self
    }

    pub fn database(&self) -> &str {
        &self.database
    }

    pub fn application(&self) -> &str {
        &self.application
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    /// Builds the C options object. Ownership passes to `bugsplat_init`.
    pub(crate) fn to_native(
        &self,
        helpers: Option<(&Path, &Path)>,
    ) -> Result<*mut sys::bugsplat_options, Error> {
        let database = CString::new(self.database.as_str())?;
        let application = CString::new(self.application.as_str())?;
        let version = CString::new(self.version.as_str())?;
        // Validate every string before allocating the C object so errors cannot leak it.
        let strings = NativeStrings::new(self, helpers)?;

        unsafe {
            let o = sys::bugsplat_options_new(database.as_ptr(), application.as_ptr(), version.as_ptr());
            if o.is_null() {
                return Err(Error::Internal);
            }
            sys::bugsplat_options_set_upload_policy(
                o,
                match self.upload_policy {
                    UploadPolicy::Dialog => sys::bugsplat_upload_policy::BUGSPLAT_UPLOAD_DIALOG,
                    UploadPolicy::Quiet => sys::bugsplat_upload_policy::BUGSPLAT_UPLOAD_QUIET,
                    UploadPolicy::Manual => sys::bugsplat_upload_policy::BUGSPLAT_UPLOAD_MANUAL,
                },
            );
            sys::bugsplat_options_set_dump_type(
                o,
                match self.dump_type {
                    DumpType::Normal => sys::bugsplat_dump_type::BUGSPLAT_DUMP_NORMAL,
                    DumpType::Heap => sys::bugsplat_dump_type::BUGSPLAT_DUMP_HEAP,
                    DumpType::Full => sys::bugsplat_dump_type::BUGSPLAT_DUMP_FULL,
                },
            );
            if let Some((timeout, policy)) = self.hang {
                let ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX).max(1);
                sys::bugsplat_options_set_hang_detection(
                    o,
                    ms,
                    match policy {
                        HangPolicy::Report => sys::bugsplat_hang_policy::BUGSPLAT_HANG_REPORT,
                        HangPolicy::ReportAndTerminate => {
                            sys::bugsplat_hang_policy::BUGSPLAT_HANG_REPORT_AND_TERMINATE
                        }
                    },
                );
            }
            if let Some(open) = self.open_support_url {
                sys::bugsplat_options_set_open_support_url(o, open as i32);
            }
            let s = &strings;
            if let Some(v) = &s.store_dir {
                sys::bugsplat_options_set_store_dir(o, v.as_ptr());
            }
            if let Some(v) = &s.monitor {
                sys::bugsplat_options_set_monitor_path(o, v.as_ptr());
            }
            if let Some(v) = &s.reporter {
                sys::bugsplat_options_set_reporter_path(o, v.as_ptr());
            }
            if let Some(v) = &s.key {
                sys::bugsplat_options_set_key(o, v.as_ptr());
            }
            if let Some(v) = &s.user {
                sys::bugsplat_options_set_user(o, v.as_ptr());
            }
            if let Some(v) = &s.email {
                sys::bugsplat_options_set_email(o, v.as_ptr());
            }
            if let Some(v) = &s.description {
                sys::bugsplat_options_set_user_description(o, v.as_ptr());
            }
            if let Some(v) = &s.notes {
                sys::bugsplat_options_set_notes(o, v.as_ptr());
            }
            if let Some(v) = &s.environment {
                sys::bugsplat_options_set_environment(o, v.as_ptr());
            }
            for (name, value) in &s.attributes {
                sys::bugsplat_options_set_attribute(o, name.as_ptr(), value.as_ptr());
            }
            for path in &s.attachments {
                sys::bugsplat_options_add_attachment(o, path.as_ptr());
            }
            Ok(o)
        }
    }
}

struct NativeStrings {
    store_dir: Option<CString>,
    monitor: Option<CString>,
    reporter: Option<CString>,
    key: Option<CString>,
    user: Option<CString>,
    email: Option<CString>,
    description: Option<CString>,
    notes: Option<CString>,
    environment: Option<CString>,
    attributes: Vec<(CString, CString)>,
    attachments: Vec<CString>,
}

impl NativeStrings {
    fn new(o: &Options, helpers: Option<(&Path, &Path)>) -> Result<Self, Error> {
        let opt = |s: &Option<String>| s.as_deref().map(CString::new).transpose();
        let path = |p: Option<&Path>| p.map(path_cstring).transpose();
        let monitor = o.monitor_path.as_deref().or(helpers.map(|h| h.0));
        let reporter = o.reporter_path.as_deref().or(helpers.map(|h| h.1));
        Ok(NativeStrings {
            store_dir: path(o.store_dir.as_deref())?,
            monitor: path(monitor)?,
            reporter: path(reporter)?,
            key: opt(&o.key)?,
            user: opt(&o.user)?,
            email: opt(&o.email)?,
            description: opt(&o.description)?,
            notes: opt(&o.notes)?,
            environment: opt(&o.environment)?,
            attributes: o
                .attributes
                .iter()
                .map(|(k, v)| Ok((CString::new(k.as_str())?, CString::new(v.as_str())?)))
                .collect::<Result<_, Error>>()?,
            attachments: o.attachments.iter().map(|p| path_cstring(p)).collect::<Result<_, _>>()?,
        })
    }
}

/// The C API takes UTF-8 paths on every platform.
pub(crate) fn path_cstring(path: &Path) -> Result<CString, Error> {
    let s = path.to_str().ok_or(Error::InvalidArgument)?;
    Ok(CString::new(s)?)
}
