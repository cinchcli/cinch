//! Shared key-exchange responder logic. The desktop's sync writer and
//! `cinch pull --watch` both invoke `handle_event` when the relay
//! broadcasts `key_exchange_requested` for a peer device that has
//! registered a public key but lacks an encrypted bundle.

use crate::crypto;
use crate::http::{HttpError, RestClient};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

#[derive(Debug, thiserror::Error)]
pub enum RespondError {
    #[error("derive shared key: {0}")]
    DeriveShared(String),
    #[error("encrypt user key: {0}")]
    Encrypt(String),
    #[error("post bundle: {0}")]
    Post(#[from] HttpError),
}

#[derive(Debug, thiserror::Error)]
pub enum HandleEventError {
    #[error("list devices: {0}")]
    ListDevices(HttpError),
    #[error("peer device {0} not found in list_devices response")]
    PeerNotFound(String),
    #[error("peer device {0} has no public key registered")]
    PeerPubkeyMissing(String),
    #[error("respond: {0}")]
    Respond(#[from] RespondError),
}

/// Build and post an encrypted key bundle for `target_device_id`.
///
/// `user_master_key_b64` is the local device's stored encryption key
/// (`base64url(32-byte AES-256 secret)`). `peer_pub_b64` comes from
/// the WS event payload; the relay vouches for its origin.
pub async fn respond(
    client: &RestClient,
    target_device_id: &str,
    peer_pub_b64: &str,
    user_master_key_b64: &str,
) -> Result<(), RespondError> {
    let (eph_priv_b64, eph_pub_b64) = crypto::generate_ephemeral_keypair();

    let shared = crypto::derive_shared_key(&eph_priv_b64, peer_pub_b64)
        .map_err(RespondError::DeriveShared)?;

    let raw_master = URL_SAFE_NO_PAD
        .decode(user_master_key_b64)
        .map_err(|e| RespondError::Encrypt(format!("master key decode: {}", e)))?;

    let encrypted = crypto::encrypt(&shared, &raw_master).map_err(RespondError::Encrypt)?;

    client
        .post_key_bundle(target_device_id, &eph_pub_b64, &encrypted)
        .await?;
    Ok(())
}

/// End-to-end handler for a `KeyExchangeRequested` WS event: look up
/// the peer device's public key via `list_devices`, then call
/// `respond` to post the encrypted bundle.
///
/// Callers (`Writer`, `cinch pull --watch`) get the 32-byte master
/// key from the local credstore or directly from `WsConfig`. Pass it
/// in raw — base64url encoding happens here.
pub async fn handle_event(
    client: &RestClient,
    target_device_id: &str,
    user_master_key: &[u8; 32],
) -> Result<(), HandleEventError> {
    let devices = client
        .list_devices()
        .await
        .map_err(HandleEventError::ListDevices)?;
    let peer = devices
        .iter()
        .find(|d| d.id == target_device_id)
        .ok_or_else(|| HandleEventError::PeerNotFound(target_device_id.to_string()))?;
    if peer.public_key.is_empty() {
        return Err(HandleEventError::PeerPubkeyMissing(
            target_device_id.to_string(),
        ));
    }
    let key_b64 = URL_SAFE_NO_PAD.encode(user_master_key);
    respond(client, target_device_id, &peer.public_key, &key_b64).await?;
    Ok(())
}
