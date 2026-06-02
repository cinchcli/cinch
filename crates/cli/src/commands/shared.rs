//! Helpers shared across `cinch` subcommands.

use client_core::protocol::DeviceInfo;

/// Resolves a device nickname/hostname to its `source_key`.
///
/// Matching is case-insensitive against both `nickname` (when non-empty) and
/// `hostname`. Falls back to `remote:<from>` when no device matches. Shared by
/// `list` (operates on a pre-fetched slice) and `pull` (fetches, then matches).
pub fn match_device_source(devices: &[DeviceInfo], from: &str) -> String {
    let lower = from.to_lowercase();
    for d in devices {
        let nick_match = !d.nickname.is_empty() && d.nickname.to_lowercase() == lower;
        let host_match = d.hostname.to_lowercase() == lower;
        if nick_match || host_match {
            return d.source_key.clone();
        }
    }
    format!("remote:{}", from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_nickname_case_insensitive() {
        let dev = DeviceInfo {
            nickname: "Desktop".into(),
            hostname: "host-1".into(),
            source_key: "remote:dev-abc".into(),
            ..Default::default()
        };
        assert_eq!(match_device_source(&[dev], "desktop"), "remote:dev-abc");
    }

    #[test]
    fn falls_back_to_remote_prefix() {
        let devices: Vec<DeviceInfo> = vec![];
        assert_eq!(match_device_source(&devices, "ghost"), "remote:ghost");
    }
}
