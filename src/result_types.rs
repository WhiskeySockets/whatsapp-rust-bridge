//! Typed return values for wasm-bindgen exported methods.
//!
//! Using `#[derive(Tsify, Serialize)]` auto-generates TypeScript types
//! and eliminates manual `js_sys::Object` construction + `skip_typescript`.

use serde::Serialize;
use tsify_next::Tsify;

/// Result from `updateProfilePicture` or `removeProfilePicture`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePictureResult {
    pub id: String,
}

/// Result from `profilePictureUrl`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePictureInfo {
    pub id: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direct_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

/// A single entry from `fetchBlocklist`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct BlocklistEntryResult {
    pub jid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
}

/// A single entry from `fetchUserInfo`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UserInfoResult {
    pub jid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_id: Option<String>,
    pub is_business: bool,
}

/// A participant change result from `groupParticipantsUpdate`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantChangeResult {
    pub jid: String,
    pub status: String,
}

/// A single media host from `getMediaConn`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct MediaHost {
    pub hostname: String,
}

/// Result from `getMediaConn`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct MediaConnResult {
    pub auth: String,
    pub ttl: f64,
    pub hosts: Vec<MediaHost>,
}

/// Result from `uploadMedia`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UploadMediaResult {
    pub url: String,
    pub direct_path: String,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub media_key: Vec<u8>,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub file_sha256: Vec<u8>,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub file_enc_sha256: Vec<u8>,
    pub file_length: f64,
}

/// Result from `encryptMediaStream`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct EncryptMediaResult {
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub media_key: Vec<u8>,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub file_sha256: Vec<u8>,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub file_enc_sha256: Vec<u8>,
    pub file_length: f64,
}
