//! Structured exit errors mirroring `cinch/cmd/internal/exit/codes.go`.
//!
//! Codes are stable: shell scripts and CI gates depend on the exact values.

use std::fmt;

#[allow(dead_code)]
pub const SUCCESS: i32 = 0;
pub const GENERIC_ERROR: i32 = 1;
pub const AUTH_FAILURE: i32 = 2;
pub const NETWORK_ERROR: i32 = 3;
pub const RELAY_ERROR: i32 = 4;
pub const ENCRYPTION_REQUIRED: i32 = 5;
/// Master AES key has not yet arrived from a paired device via ECDH. Unlike
/// `ENCRYPTION_REQUIRED` (which tells the user to re-login), this code
/// signals that the device is correctly signed in but waiting on the
/// key-exchange handshake. Recovery: `cinch auth retry-key`.
pub const ENCRYPTION_PENDING: i32 = 6;

#[derive(Debug)]
pub struct ExitError {
    pub code: i32,
    pub message: String,
    pub fix: String,
}

impl ExitError {
    pub fn new(code: i32, message: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            fix: fix.into(),
        }
    }

    pub fn print_stderr(&self) {
        eprintln!("\u{2717} {}", self.message);
        if !self.fix.is_empty() {
            eprintln!("  {}", self.fix);
        }
    }
}

impl fmt::Display for ExitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ExitError {}

impl From<client_core::http::HttpError> for ExitError {
    fn from(err: client_core::http::HttpError) -> Self {
        use client_core::http::HttpError;
        match err {
            HttpError::Unauthorized => ExitError::new(
                AUTH_FAILURE,
                "Authentication required.",
                "Run: cinch auth login",
            ),
            HttpError::Network(msg) => ExitError::new(
                NETWORK_ERROR,
                format!("Relay unreachable: {}", msg),
                "Check your connection or try again later.",
            ),
            HttpError::Relay {
                status,
                message,
                fix,
            } => ExitError::new(
                RELAY_ERROR,
                format!("Relay error ({}): {}", status, message),
                if fix.is_empty() {
                    "If this persists, check relay status.".to_string()
                } else {
                    fix
                },
            ),
            HttpError::Decode(msg) => ExitError::new(
                RELAY_ERROR,
                format!("Could not decode relay response: {}", msg),
                String::new(),
            ),
            HttpError::Build(msg) => ExitError::new(
                GENERIC_ERROR,
                format!("Could not build request: {}", msg),
                String::new(),
            ),
        }
    }
}
