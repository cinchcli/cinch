//! Self-version reporting helpers shared by CLI and desktop.
//!
//! Both clients pass their own CARGO_PKG_VERSION (binary-crate level, not
//! cinchcli-core's version) into `ClientInfo` at startup. The resulting
//! struct is consumed by RestClient (HTTP headers) and WsClient (hello).

use reqwest::header::{HeaderName, HeaderValue};

use crate::protocol::{ClientHelloPayload, WSMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientType {
    Cli,
    Desktop,
}

impl ClientType {
    pub fn as_str(self) -> &'static str {
        match self {
            ClientType::Cli => "cli",
            ClientType::Desktop => "desktop",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub client_type: ClientType,
    pub version: String,
}

pub const HEADER_CLIENT_VERSION: &str = "x-cinch-client-version";
pub const HEADER_CLIENT_TYPE: &str = "x-cinch-client-type";

impl ClientInfo {
    pub fn http_headers(&self) -> [(HeaderName, HeaderValue); 2] {
        [
            (
                HeaderName::from_static(HEADER_CLIENT_VERSION),
                HeaderValue::from_str(&self.version).expect("ascii semver"),
            ),
            (
                HeaderName::from_static(HEADER_CLIENT_TYPE),
                HeaderValue::from_static(self.client_type.as_str()),
            ),
        ]
    }

    pub fn client_hello_message(&self) -> WSMessage {
        WSMessage {
            action: crate::protocol::ACTION_CLIENT_HELLO.to_string(),
            client_hello: Some(ClientHelloPayload {
                version: self.version.clone(),
                type_: self.client_type.as_str().to_string(),
                os: std::env::consts::OS.to_string(),
            }),
            ..Default::default()
        }
    }

    /// Test-only helper. Returns a `ClientInfo` suitable for wiring through
    /// integration and unit tests that exercise `RestClient::new` and
    /// `WsConfig` without caring about a real CLI / desktop version.
    pub fn for_test() -> Self {
        Self {
            client_type: ClientType::Cli,
            version: "0.0.0-test".to_string(),
        }
    }
}
