//! Full WhatsApp client running in WASM.
//!
//! Wraps `whatsapp_rust::Client` with JS-provided adapters for
//! transport (WebSocket), storage (InMemory/JS), and HTTP (fetch).

use std::sync::Arc;

use log::info;
use wacore::types::events::{Event, EventHandler};
use wacore_binary::jid::Jid;
use wasm_bindgen::prelude::*;

use crate::js_backend;
use crate::js_http::JsHttpClientAdapter;
use crate::js_time;
use crate::js_transport::JsTransportFactory;
use crate::runtime::WasmRuntime;

// ---------------------------------------------------------------------------
// TypeScript type declarations
// ---------------------------------------------------------------------------

#[wasm_bindgen(typescript_custom_section)]
const TS_EVENT: &str = r#"
export type WhatsAppEvent =
  | { type: 'connected'; data: Record<string, never> }
  | { type: 'disconnected'; data: Record<string, never> }
  | { type: 'qr'; data: { code: string; timeout: number } }
  | { type: 'pairing_code'; data: { code: string; timeout: number } }
  | { type: 'pair_success'; data: { id: string; lid: string; businessName: string; platform: string } }
  | { type: 'pair_error'; data: { id: string; lid: string; businessName: string; platform: string; error: string } }
  | { type: 'logged_out'; data: { onConnect: boolean; reason: string } }
  | { type: 'message'; data: { message: any; info: any } }
  | { type: 'receipt'; data: any }
  | { type: 'undecryptable_message'; data: any }
  | { type: 'notification'; data: any }
  | { type: 'chat_presence'; data: any }
  | { type: 'presence'; data: any }
  | { type: 'picture_update'; data: any }
  | { type: 'user_about_update'; data: any }
  | { type: 'contact_updated'; data: any }
  | { type: 'contact_number_changed'; data: any }
  | { type: 'contact_sync_requested'; data: any }
  | { type: 'joined_group'; data: any }
  | { type: 'group_update'; data: any }
  | { type: 'contact_update'; data: any }
  | { type: 'push_name_update'; data: any }
  | { type: 'self_push_name_updated'; data: any }
  | { type: 'pin_update'; data: any }
  | { type: 'mute_update'; data: any }
  | { type: 'archive_update'; data: any }
  | { type: 'star_update'; data: any }
  | { type: 'mark_chat_as_read_update'; data: any }
  | { type: 'history_sync'; data: any }
  | { type: 'offline_sync_preview'; data: any }
  | { type: 'offline_sync_completed'; data: any }
  | { type: 'device_list_update'; data: any }
  | { type: 'business_status_update'; data: any }
  | { type: 'stream_replaced'; data: Record<string, never> }
  | { type: 'temporary_ban'; data: any }
  | { type: 'connect_failure'; data: any }
  | { type: 'stream_error'; data: any }
  | { type: 'disappearing_mode_changed'; data: any }
  | { type: 'newsletter_live_update'; data: any }
  | { type: 'qr_scanned_without_multidevice'; data: Record<string, never> }
  | { type: 'client_outdated'; data: Record<string, never> };

export interface WhatsAppClientConfig {
  transport: JsTransportCallbacks;
  httpClient: JsHttpClientConfig;
  onEvent?: (event: WhatsAppEvent) => void;
}
"#;

// ---------------------------------------------------------------------------
// JS event handler bridge
// ---------------------------------------------------------------------------

/// Bridges Rust events to a JS callback function via an ordered channel.
///
/// Events are sent through an async channel and dispatched by a single
/// consumer loop, which guarantees delivery order (unlike per-event
/// `spawn_local` which does not).
struct JsEventHandler {
    event_tx: async_channel::Sender<JsValue>,
}

crate::wasm_send_sync!(JsEventHandler);

impl JsEventHandler {
    fn new(callback: js_sys::Function) -> Self {
        let (event_tx, event_rx) = async_channel::bounded(4096);

        // Single consumer loop — guarantees event ordering
        wasm_bindgen_futures::spawn_local(async move {
            while let Ok(event) = event_rx.recv().await {
                if let Err(e) = callback.call1(&JsValue::NULL, &event) {
                    log::warn!("JS event callback threw: {:?}", e);
                }
            }
        });

        Self { event_tx }
    }
}

impl EventHandler for JsEventHandler {
    fn handle_event(&self, event: &Event) {
        match event_to_js(event) {
            Ok(js_event) => {
                if let Err(e) = self.event_tx.try_send(js_event) {
                    log::warn!("Event channel send failed: {e}");
                }
            }
            Err(e) => log::warn!("Event serialization failed: {e:?}"),
        }
    }
}

/// Helper macro for Event variants whose payload is directly serializable
/// via `crate::proto::to_js_value`.
macro_rules! serialize_event {
    ($event:expr, { $( $variant:ident => $name:literal ),* $(,)? }) => {
        match $event {
            $( Event::$variant(data) => ($name, crate::proto::to_js_value(data)?), )*
            other => return event_to_js_special(other),
        }
    };
}

/// Convert a Rust Event to a JS object `{ type: string, data: any }`.
fn event_to_js(event: &Event) -> Result<JsValue, JsValue> {
    let obj = js_sys::Object::new();

    // Common case: payload implements Serialize, just use to_js_value
    let (event_type, data) = serialize_event!(event, {
        Receipt                 => "receipt",
        UndecryptableMessage    => "undecryptable_message",
        Notification            => "notification",
        ChatPresence            => "chat_presence",
        Presence                => "presence",
        PictureUpdate           => "picture_update",
        UserAboutUpdate         => "user_about_update",
        ContactUpdated          => "contact_updated",
        ContactNumberChanged    => "contact_number_changed",
        ContactSyncRequested    => "contact_sync_requested",
        GroupUpdate             => "group_update",
        ContactUpdate           => "contact_update",
        PushNameUpdate          => "push_name_update",
        SelfPushNameUpdated     => "self_push_name_updated",
        PinUpdate               => "pin_update",
        MuteUpdate              => "mute_update",
        ArchiveUpdate           => "archive_update",
        StarUpdate              => "star_update",
        MarkChatAsReadUpdate    => "mark_chat_as_read_update",
        HistorySync             => "history_sync",
        OfflineSyncPreview      => "offline_sync_preview",
        OfflineSyncCompleted    => "offline_sync_completed",
        DeviceListUpdate        => "device_list_update",
        BusinessStatusUpdate    => "business_status_update",
        TemporaryBan            => "temporary_ban",
        ConnectFailure          => "connect_failure",
        StreamError             => "stream_error",
        DisappearingModeChanged => "disappearing_mode_changed",
        NewsletterLiveUpdate    => "newsletter_live_update",
    });

    js_sys::Reflect::set(&obj, &"type".into(), &event_type.into())?;
    js_sys::Reflect::set(&obj, &"data".into(), &data)?;
    Ok(obj.into())
}

/// Handles Event variants that need special serialization (no data, named
/// fields, multi-field payloads, or pre-processing).
fn event_to_js_special(event: &Event) -> Result<JsValue, JsValue> {
    let obj = js_sys::Object::new();
    let empty = || JsValue::from(js_sys::Object::new());

    let (event_type, data) = match event {
        Event::Connected(_) => ("connected", empty()),
        Event::Disconnected(_) => ("disconnected", empty()),
        Event::QrScannedWithoutMultidevice(_) => ("qr_scanned_without_multidevice", empty()),
        Event::ClientOutdated(_) => ("client_outdated", empty()),
        Event::StreamReplaced(_) => ("stream_replaced", empty()),
        Event::PairingQrCode { code, timeout } => {
            let d = js_sys::Object::new();
            js_sys::Reflect::set(&d, &"code".into(), &code.into())?;
            js_sys::Reflect::set(&d, &"timeout".into(), &(timeout.as_secs() as f64).into())?;
            ("qr", d.into())
        }
        Event::PairingCode { code, timeout } => {
            let d = js_sys::Object::new();
            js_sys::Reflect::set(&d, &"code".into(), &code.into())?;
            js_sys::Reflect::set(&d, &"timeout".into(), &(timeout.as_secs() as f64).into())?;
            ("pairing_code", d.into())
        }
        Event::PairSuccess(ps) => {
            let d = js_sys::Object::new();
            js_sys::Reflect::set(&d, &"id".into(), &ps.id.to_string().into())?;
            js_sys::Reflect::set(&d, &"lid".into(), &ps.lid.to_string().into())?;
            js_sys::Reflect::set(
                &d,
                &"businessName".into(),
                &ps.business_name.as_str().into(),
            )?;
            js_sys::Reflect::set(&d, &"platform".into(), &ps.platform.as_str().into())?;
            ("pair_success", d.into())
        }
        Event::PairError(pe) => {
            let d = js_sys::Object::new();
            js_sys::Reflect::set(&d, &"id".into(), &pe.id.to_string().into())?;
            js_sys::Reflect::set(&d, &"lid".into(), &pe.lid.to_string().into())?;
            js_sys::Reflect::set(
                &d,
                &"businessName".into(),
                &pe.business_name.as_str().into(),
            )?;
            js_sys::Reflect::set(&d, &"platform".into(), &pe.platform.as_str().into())?;
            js_sys::Reflect::set(&d, &"error".into(), &pe.error.as_str().into())?;
            ("pair_error", d.into())
        }
        Event::LoggedOut(lo) => {
            let d = js_sys::Object::new();
            js_sys::Reflect::set(&d, &"onConnect".into(), &lo.on_connect.into())?;
            js_sys::Reflect::set(&d, &"reason".into(), &format!("{:?}", lo.reason).into())?;
            ("logged_out", d.into())
        }
        Event::Message(msg, info) => {
            let d = js_sys::Object::new();
            js_sys::Reflect::set(
                &d,
                &"message".into(),
                &crate::proto::to_js_value(msg.as_ref())?,
            )?;
            js_sys::Reflect::set(&d, &"info".into(), &crate::proto::to_js_value(info)?)?;
            ("message", d.into())
        }
        Event::JoinedGroup(lg) => {
            // Force parse before serialization; log if parsing fails
            if lg.get().is_none() {
                log::warn!("Failed to parse JoinedGroup conversation from raw bytes");
            }
            ("joined_group", crate::proto::to_js_value(lg)?)
        }
        // All other variants are handled by serialize_event! in event_to_js
        _ => unreachable!("unhandled event variant in event_to_js_special"),
    };

    js_sys::Reflect::set(&obj, &"type".into(), &event_type.into())?;
    js_sys::Reflect::set(&obj, &"data".into(), &data)?;
    Ok(obj.into())
}

/// Default WhatsApp Web version. Hardcoded to skip HTTP version-check on startup.
const DEFAULT_WA_WEB_VERSION: (u32, u32, u32) = (2, 3000, 1031424117);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the WASM environment. Must be called once before creating clients.
///
/// Sets up:
/// - Panic hook: Rust panics are logged to console.error with full messages
/// - Logger: Rust `log::info!`, `log::warn!`, etc. go to console.log
/// - Time provider: Uses JS `Date.now()` for timestamps
#[wasm_bindgen(js_name = initWasmEngine)]
pub fn init_wasm_engine() {
    // Only init once (all three are idempotent or check internally)
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Debug);
    js_time::init_time_provider();
}

// ---------------------------------------------------------------------------
// Client creation
// ---------------------------------------------------------------------------

/// A full WhatsApp client running in WASM.
///
/// Usage from JS:
/// ```js
/// initWasmEngine();
/// const client = await createWhatsAppClient(transportConfig, httpConfig, onEvent);
/// await client.run();
/// ```
#[wasm_bindgen(js_name = createWhatsAppClient)]
pub async fn create_whatsapp_client(
    transport_config: JsValue,
    http_config: JsValue,
    on_event: Option<js_sys::Function>,
) -> Result<WasmWhatsAppClient, JsValue> {
    let runtime = Arc::new(WasmRuntime) as Arc<dyn wacore::runtime::Runtime>;
    let backend = js_backend::new_in_memory_backend();
    let transport_factory = Arc::new(JsTransportFactory::from_js(transport_config)?)
        as Arc<dyn wacore::net::TransportFactory>;
    let http_client =
        Arc::new(JsHttpClientAdapter::from_js(http_config)?) as Arc<dyn wacore::net::HttpClient>;

    // Create persistence manager
    let persistence_manager: Arc<whatsapp_rust::store::persistence_manager::PersistenceManager> =
        Arc::new(
            whatsapp_rust::store::persistence_manager::PersistenceManager::new(backend.clone())
                .await
                .map_err(|e| JsValue::from_str(&format!("create persistence manager: {e}")))?,
        );

    // Create the client
    let (client, sync_rx) = whatsapp_rust::Client::new_with_cache_config(
        runtime.clone(),
        persistence_manager,
        transport_factory,
        http_client,
        Some(DEFAULT_WA_WEB_VERSION),
        whatsapp_rust::CacheConfig::default(),
    )
    .await;

    // Register JS event handler if provided
    if let Some(callback) = on_event {
        let handler = Arc::new(JsEventHandler::new(callback)) as Arc<dyn EventHandler>;
        client.register_handler(handler);
    }

    Ok(WasmWhatsAppClient {
        client,
        runtime,
        sync_rx: Some(sync_rx),
    })
}

// ---------------------------------------------------------------------------
// Client wrapper
// ---------------------------------------------------------------------------

/// Opaque handle to the WhatsApp client.
#[wasm_bindgen]
pub struct WasmWhatsAppClient {
    client: Arc<whatsapp_rust::Client>,
    #[allow(dead_code)]
    runtime: Arc<dyn wacore::runtime::Runtime>,
    sync_rx: Option<async_channel::Receiver<whatsapp_rust::sync_task::MajorSyncTask>>,
}

#[wasm_bindgen]
impl WasmWhatsAppClient {
    // ── Connection ───────────────────────────────────────────────────────

    /// Run the main client loop (connect, handshake, message processing).
    ///
    /// This starts the sync worker, then enters the client's reconnect loop.
    /// Returns when the client is intentionally disconnected.
    /// Start the main client loop in the background.
    ///
    /// This spawns the connection loop (connect → handshake → message loop → reconnect)
    /// as a background task and returns immediately. The loop runs until `disconnect()`
    /// is called.
    ///
    /// This is NOT `async` — it returns synchronously to avoid holding a wasm-bindgen
    /// borrow on `self` that would prevent calling other methods (disconnect, etc.).
    pub fn run(&mut self) -> Result<(), JsValue> {
        if self.sync_rx.is_none() {
            return Err(JsValue::from_str("run() has already been called"));
        }
        let client = self.client.clone();
        let runtime = self.runtime.clone();
        let sync_rx = self.sync_rx.take();

        // Start sync worker drain loop
        if let Some(receiver) = sync_rx {
            runtime
                .spawn(Box::pin(async move {
                    while let Ok(task) = receiver.recv().await {
                        info!("Sync task received: {:?}", std::mem::discriminant(&task),);
                    }
                }))
                .detach();
        }

        // Spawn the main loop as a background task — does NOT hold &self
        runtime
            .spawn(Box::pin(async move {
                client.run().await;
                info!("Client run loop exited.");
            }))
            .detach();

        Ok(())
    }

    /// Connect to WhatsApp servers (single connection, no auto-reconnect).
    pub async fn connect(&self) -> Result<(), JsValue> {
        self.client
            .connect()
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    /// Disconnect the client.
    pub async fn disconnect(&self) {
        self.client.disconnect().await;
    }

    /// Check if the client is connected.
    #[wasm_bindgen(js_name = isConnected)]
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }

    /// Check if the client is logged in (paired).
    #[wasm_bindgen(js_name = isLoggedIn)]
    pub fn is_logged_in(&self) -> bool {
        self.client.is_logged_in()
    }

    // ── Sending messages ─────────────────────────────────────────────────

    /// Send an end-to-end encrypted message.
    ///
    /// `jid` is the recipient as a string (e.g. `"5511999999999@s.whatsapp.net"`).
    /// `message` is a JS object matching the Message protobuf schema.
    /// Returns the message ID string on success.
    #[wasm_bindgen(js_name = sendMessage)]
    pub async fn send_message(&self, jid: &str, message: JsValue) -> Result<JsValue, JsValue> {
        let to: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;
        let msg: waproto::whatsapp::Message = serde_wasm_bindgen::from_value(message)
            .map_err(|e| JsValue::from_str(&format!("invalid message: {e}")))?;

        let message_id = self
            .client
            .send_message(to, msg)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        Ok(JsValue::from_str(&message_id))
    }

    // ── Groups ───────────────────────────────────────────────────────────

    /// Get metadata for a group.
    #[wasm_bindgen(js_name = getGroupMetadata)]
    pub async fn group_metadata(&self, jid: &str) -> Result<JsValue, JsValue> {
        let group_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let metadata = self
            .client
            .groups()
            .get_metadata(&group_jid)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        group_metadata_to_js(&metadata)
    }

    /// Create a new group.
    ///
    /// Returns an object with `{ gid: string }`.
    #[wasm_bindgen(js_name = createGroup)]
    pub async fn group_create(
        &self,
        subject: &str,
        participants: Vec<String>,
    ) -> Result<JsValue, JsValue> {
        use whatsapp_rust::features::GroupParticipantOptions;

        let participant_options: Vec<GroupParticipantOptions> = participants
            .iter()
            .map(|p| {
                let jid: Jid = p
                    .parse()
                    .unwrap_or_else(|_| Jid::new(p, wacore_binary::jid::DEFAULT_USER_SERVER));
                GroupParticipantOptions::new(jid)
            })
            .collect();

        let options = whatsapp_rust::features::GroupCreateOptions::new(subject)
            .with_participants(participant_options);

        let result = self
            .client
            .groups()
            .create_group(options)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"gid".into(), &result.gid.to_string().into())?;
        Ok(obj.into())
    }

    // ── Contacts ─────────────────────────────────────────────────────────

    /// Check if a phone number is registered on WhatsApp.
    ///
    /// Returns an array of `{ jid: string, isRegistered: boolean }`.
    #[wasm_bindgen(js_name = isOnWhatsApp)]
    pub async fn is_on_whatsapp(&self, phone: &str) -> Result<JsValue, JsValue> {
        let results = self
            .client
            .contacts()
            .is_on_whatsapp(&[phone])
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        let arr = js_sys::Array::new();
        for r in &results {
            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&obj, &"jid".into(), &r.jid.to_string().into())?;
            js_sys::Reflect::set(&obj, &"isRegistered".into(), &r.is_registered.into())?;
            arr.push(&obj.into());
        }
        Ok(arr.into())
    }

    // ── Profile ──────────────────────────────────────────────────────────

    /// Set the user's push name (display name).
    #[wasm_bindgen(js_name = setPushName)]
    pub async fn set_push_name(&self, name: &str) -> Result<(), JsValue> {
        self.client
            .profile()
            .set_push_name(name)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    // ── Presence ─────────────────────────────────────────────────────────

    /// Send presence status ("available" or "unavailable").
    #[wasm_bindgen(js_name = sendPresence)]
    pub async fn send_presence(&self, status: &str) -> Result<(), JsValue> {
        let presence_status = match status {
            "available" => whatsapp_rust::features::PresenceStatus::Available,
            "unavailable" => whatsapp_rust::features::PresenceStatus::Unavailable,
            _ => {
                return Err(JsValue::from_str(
                    "status must be 'available' or 'unavailable'",
                ));
            }
        };

        self.client
            .presence()
            .set(presence_status)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    // ── Chat state ───────────────────────────────────────────────────────

    /// Send a chat state update (typing indicator).
    ///
    /// `state` must be one of: "composing", "recording", "paused".
    #[wasm_bindgen(js_name = sendChatState)]
    pub async fn send_chat_state(&self, jid: &str, state: &str) -> Result<(), JsValue> {
        let to: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let chat_state = match state {
            "composing" => whatsapp_rust::features::ChatStateType::Composing,
            "recording" => whatsapp_rust::features::ChatStateType::Recording,
            "paused" => whatsapp_rust::features::ChatStateType::Paused,
            _ => {
                return Err(JsValue::from_str(
                    "state must be 'composing', 'recording', or 'paused'",
                ));
            }
        };

        self.client
            .chatstate()
            .send(&to, chat_state)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    // ── State getters ────────────────────────────────────────────────────

    /// Get the current push name.
    #[wasm_bindgen(js_name = getPushName)]
    pub async fn get_push_name(&self) -> String {
        self.client.get_push_name().await
    }

    /// Get the own JID (phone number JID) if logged in.
    #[wasm_bindgen(js_name = getJid)]
    pub async fn get_jid(&self) -> Option<String> {
        self.client.get_pn().await.map(|j| j.to_string())
    }

    /// Get the own LID (linked identity) if available.
    #[wasm_bindgen(js_name = getLid)]
    pub async fn get_lid(&self) -> Option<String> {
        self.client.get_lid().await.map(|j| j.to_string())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert GroupMetadata to a JS object (GroupMetadata doesn't derive Serialize).
fn group_metadata_to_js(
    metadata: &whatsapp_rust::features::GroupMetadata,
) -> Result<JsValue, JsValue> {
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"id".into(), &metadata.id.to_string().into())?;
    js_sys::Reflect::set(&obj, &"subject".into(), &metadata.subject.as_str().into())?;

    // Participants
    let parts = js_sys::Array::new();
    for p in &metadata.participants {
        let po = js_sys::Object::new();
        js_sys::Reflect::set(&po, &"jid".into(), &p.jid.to_string().into())?;
        js_sys::Reflect::set(
            &po,
            &"phoneNumber".into(),
            &match &p.phone_number {
                Some(pn) => JsValue::from_str(&pn.to_string()),
                None => JsValue::NULL,
            },
        )?;
        js_sys::Reflect::set(&po, &"isAdmin".into(), &p.is_admin.into())?;
        parts.push(&po.into());
    }
    js_sys::Reflect::set(&obj, &"participants".into(), &parts.into())?;

    js_sys::Reflect::set(
        &obj,
        &"addressingMode".into(),
        &format!("{:?}", metadata.addressing_mode).into(),
    )?;

    // Optional fields
    set_optional_str(
        &obj,
        "creator",
        &metadata.creator.as_ref().map(|j| j.to_string()),
    )?;
    set_optional_num(
        &obj,
        "creationTime",
        &metadata.creation_time.map(|v| v as f64),
    )?;
    set_optional_num(
        &obj,
        "subjectTime",
        &metadata.subject_time.map(|v| v as f64),
    )?;
    set_optional_str(
        &obj,
        "subjectOwner",
        &metadata.subject_owner.as_ref().map(|j| j.to_string()),
    )?;
    set_optional_str(&obj, "description", &metadata.description)?;
    set_optional_str(&obj, "descriptionId", &metadata.description_id)?;

    js_sys::Reflect::set(&obj, &"isLocked".into(), &metadata.is_locked.into())?;
    js_sys::Reflect::set(
        &obj,
        &"isAnnouncement".into(),
        &metadata.is_announcement.into(),
    )?;
    js_sys::Reflect::set(
        &obj,
        &"ephemeralExpiration".into(),
        &(metadata.ephemeral_expiration as f64).into(),
    )?;
    js_sys::Reflect::set(
        &obj,
        &"membershipApproval".into(),
        &metadata.membership_approval.into(),
    )?;
    set_optional_str(
        &obj,
        "memberAddMode",
        &metadata
            .member_add_mode
            .as_ref()
            .map(|m| format!("{:?}", m)),
    )?;
    set_optional_str(
        &obj,
        "memberLinkMode",
        &metadata
            .member_link_mode
            .as_ref()
            .map(|m| format!("{:?}", m)),
    )?;
    set_optional_num(&obj, "size", &metadata.size.map(|v| v as f64))?;

    js_sys::Reflect::set(
        &obj,
        &"isParentGroup".into(),
        &metadata.is_parent_group.into(),
    )?;
    set_optional_str(
        &obj,
        "parentGroupJid",
        &metadata.parent_group_jid.as_ref().map(|j| j.to_string()),
    )?;
    js_sys::Reflect::set(
        &obj,
        &"isDefaultSubGroup".into(),
        &metadata.is_default_sub_group.into(),
    )?;
    js_sys::Reflect::set(
        &obj,
        &"isGeneralChat".into(),
        &metadata.is_general_chat.into(),
    )?;
    js_sys::Reflect::set(
        &obj,
        &"allowNonAdminSubGroupCreation".into(),
        &metadata.allow_non_admin_sub_group_creation.into(),
    )?;

    Ok(obj.into())
}

fn set_optional_str(obj: &js_sys::Object, key: &str, val: &Option<String>) -> Result<(), JsValue> {
    let js_val = match val {
        Some(s) => JsValue::from_str(s),
        None => JsValue::NULL,
    };
    js_sys::Reflect::set(obj, &key.into(), &js_val)?;
    Ok(())
}

fn set_optional_num(obj: &js_sys::Object, key: &str, val: &Option<f64>) -> Result<(), JsValue> {
    let js_val = match val {
        Some(n) => JsValue::from_f64(*n),
        None => JsValue::NULL,
    };
    js_sys::Reflect::set(obj, &key.into(), &js_val)?;
    Ok(())
}
