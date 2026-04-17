//! Typed return values and parameter enums for wasm-bindgen exported methods.
//!
//! Using `#[derive(Tsify, Serialize)]` auto-generates TypeScript types
//! and eliminates manual `js_sys::Object` construction + `skip_typescript`.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;

// ---------------------------------------------------------------------------
// Parameter enums — typed string alternatives for &str dispatch
// ---------------------------------------------------------------------------

/// Media type for upload/download operations.
#[derive(Debug, Clone, Copy, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
pub enum MediaType {
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "video")]
    Video,
    #[serde(rename = "audio")]
    Audio,
    #[serde(rename = "document")]
    Document,
    #[serde(rename = "sticker")]
    Sticker,
    #[serde(rename = "thumbnail-link")]
    ThumbnailLink,
    #[serde(rename = "md-msg-hist")]
    History,
    #[serde(rename = "md-app-state")]
    AppState,
    /// Product catalog image — uses same crypto as Image.
    #[serde(rename = "product-catalog-image")]
    ProductCatalogImage,
}

impl From<MediaType> for wacore::download::MediaType {
    fn from(mt: MediaType) -> Self {
        match mt {
            MediaType::Image => Self::Image,
            MediaType::Video => Self::Video,
            MediaType::Audio => Self::Audio,
            MediaType::Document => Self::Document,
            MediaType::Sticker => Self::Sticker,
            MediaType::ThumbnailLink => Self::LinkThumbnail,
            MediaType::History => Self::History,
            MediaType::AppState => Self::AppState,
            MediaType::ProductCatalogImage => Self::ProductCatalogImage,
        }
    }
}

/// Block/unblock action.
#[derive(Debug, Clone, Copy, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum BlockAction {
    Block,
    Unblock,
}

/// Presence status.
#[derive(Debug, Clone, Copy, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum PresenceStatus {
    Available,
    Unavailable,
}

/// Chat state (typing indicator).
#[derive(Debug, Clone, Copy, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum ChatState {
    Composing,
    Recording,
    Paused,
}

/// Group participant action.
#[derive(Debug, Clone, Copy, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum GroupParticipantAction {
    Add,
    Remove,
    Promote,
    Demote,
}

/// Group setting type.
#[derive(Debug, Clone, Copy, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum GroupSetting {
    Locked,
    Announce,
    MembershipApproval,
}

/// Group member add mode.
#[derive(Debug, Clone, Copy, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum MemberAddMode {
    AdminAdd,
    AllMemberAdd,
}

/// Picture type for profile picture URL.
#[derive(Debug, Clone, Copy, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum PictureType {
    Preview,
    Image,
}

/// Group join request action.
#[derive(Debug, Clone, Copy, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum GroupRequestAction {
    Approve,
    Reject,
}

// ---------------------------------------------------------------------------
// Result types — serialized return values
// ---------------------------------------------------------------------------

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
    pub media_key: [u8; 32],
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub file_sha256: [u8; 32],
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub file_enc_sha256: [u8; 32],
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

/// A single voter entry for `getAggregateVotesInPollMessage`.
#[derive(Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PollVoterEntry {
    pub voter: String,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub enc_payload: Vec<u8>,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub enc_iv: Vec<u8>,
}

/// A message key for `readMessages`.
#[derive(Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ReadMessageKey {
    pub remote_jid: String,
    pub id: String,
    #[tsify(optional)]
    pub participant: Option<String>,
}

/// Result from `createGroup`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct CreateGroupResult {
    pub gid: String,
}

/// Result from `createPoll`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CreatePollResult {
    pub message_id: String,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub message_secret: Vec<u8>,
}

/// Result from `isOnWhatsApp`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct IsOnWhatsAppResult {
    pub jid: String,
    pub is_registered: bool,
}

/// Result from `fetchStatus`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct FetchStatusResult {
    pub jid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Group participant info.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct GroupParticipantInfo {
    pub jid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    pub is_admin: bool,
}

/// Result from `getGroupMetadata`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct GroupMetadataResult {
    pub id: String,
    pub subject: String,
    pub participants: Vec<GroupParticipantInfo>,
    pub addressing_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_id: Option<String>,
    pub is_locked: bool,
    pub is_announcement: bool,
    pub ephemeral_expiration: f64,
    pub membership_approval: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_add_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_link_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    pub is_parent_group: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_group_jid: Option<String>,
    pub is_default_sub_group: bool,
    pub is_general_chat: bool,
    pub allow_non_admin_sub_group_creation: bool,
}

/// Result from newsletter methods.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct NewsletterMetadataResult {
    pub jid: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub subscriber_count: f64,
    pub verification: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_time: Option<f64>,
}

/// Result from `getMemoryDiagnostics`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDiagnosticsResult {
    pub group_cache: f64,
    pub device_registry_cache: f64,
    pub sender_key_device_cache: f64,
    pub lid_pn_lid_entries: f64,
    pub lid_pn_pn_entries: f64,
    pub retried_group_messages: f64,
    pub recent_messages: f64,
    pub message_retry_counts: f64,
    pub pdo_pending_requests: f64,
    pub session_locks: f64,
    pub chat_lanes: f64,
    pub response_waiters: f64,
    pub node_waiters: f64,
    pub pending_retries: f64,
    pub presence_subscriptions: f64,
    pub app_state_key_requests: f64,
    pub app_state_syncing: f64,
    pub signal_cache_sessions: f64,
    pub signal_cache_identities: f64,
    pub signal_cache_sender_keys: f64,
    pub chatstate_handlers: f64,
    pub custom_enc_handlers: f64,
}

/// Result from `getAggregateVotesInPollMessage`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct PollAggregateResult {
    pub name: String,
    pub voters: Vec<String>,
}

/// Result from `groupRequestParticipantsList`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct MembershipRequestResult {
    pub jid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_time: Option<f64>,
}

/// Result from `getBusinessProfile`.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct BusinessProfileResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wid: Option<String>,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub website: Vec<String>,
    pub categories: Vec<BusinessCategoryResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    pub business_hours: BusinessHoursResult,
}

/// Business category info.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct BusinessCategoryResult {
    pub id: String,
    pub name: String,
}

/// Business hours.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct BusinessHoursResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub business_config: Option<Vec<BusinessHoursConfigResult>>,
}

/// Business hours config for a day.
#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct BusinessHoursConfigResult {
    pub day_of_week: String,
    pub mode: String,
    pub open_time: f64,
    pub close_time: f64,
}
