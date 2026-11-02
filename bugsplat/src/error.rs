use bugsplat_sys::bugsplat_result as R;

/// Errors from the BugSplat SDK.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid argument")]
    InvalidArgument,
    #[error("BugSplat is already initialized in this process")]
    AlreadyInitialized,
    #[error("BugSplat is not initialized")]
    NotInitialized,
    #[error("BugSplatMonitor was not found next to the app (a packaging error)")]
    MonitorNotFound,
    #[error("BugSplatMonitor failed to start")]
    MonitorStartFailed,
    #[error("BugSplatReporter was not found next to the app (a packaging error)")]
    ReporterNotFound,
    #[error("I/O error")]
    Io,
    #[error("limit exceeded (24 attachments, 64 attributes, 20 KB per value)")]
    LimitExceeded,
    #[error("not supported on this platform")]
    Unsupported,
    #[error("network error")]
    Network,
    #[error("HTTP error")]
    Http,
    #[error("the server rejected the report")]
    Rejected,
    #[error("the user cancelled")]
    Cancelled,
    #[error("internal SDK error")]
    Internal,
    #[error("string contains an interior NUL byte")]
    Nul,
    #[error("unknown SDK result code {0}")]
    Unknown(u32),
}

impl Error {
    pub(crate) fn check(result: R) -> Result<(), Error> {
        let err = match result {
            R::BUGSPLAT_OK => return Ok(()),
            R::BUGSPLAT_ERR_INVALID_ARGUMENT => Error::InvalidArgument,
            R::BUGSPLAT_ERR_ALREADY_INITIALIZED => Error::AlreadyInitialized,
            R::BUGSPLAT_ERR_NOT_INITIALIZED => Error::NotInitialized,
            R::BUGSPLAT_ERR_MONITOR_NOT_FOUND => Error::MonitorNotFound,
            R::BUGSPLAT_ERR_MONITOR_START_FAILED => Error::MonitorStartFailed,
            R::BUGSPLAT_ERR_REPORTER_NOT_FOUND => Error::ReporterNotFound,
            R::BUGSPLAT_ERR_IO => Error::Io,
            R::BUGSPLAT_ERR_LIMIT_EXCEEDED => Error::LimitExceeded,
            R::BUGSPLAT_ERR_UNSUPPORTED => Error::Unsupported,
            R::BUGSPLAT_ERR_NETWORK => Error::Network,
            R::BUGSPLAT_ERR_HTTP => Error::Http,
            R::BUGSPLAT_ERR_REJECTED => Error::Rejected,
            R::BUGSPLAT_ERR_CANCELLED => Error::Cancelled,
            R::BUGSPLAT_ERR_INTERNAL => Error::Internal,
            R(other) => Error::Unknown(other),
        };
        Err(err)
    }
}

impl From<std::ffi::NulError> for Error {
    fn from(_: std::ffi::NulError) -> Self {
        Error::Nul
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_and_unknown_codes() {
        assert_eq!(Error::check(R::BUGSPLAT_OK), Ok(()));
        assert_eq!(Error::check(R::BUGSPLAT_ERR_MONITOR_NOT_FOUND), Err(Error::MonitorNotFound));
        assert_eq!(Error::check(R(999)), Err(Error::Unknown(999)));
    }
}
