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
  | { type: 'message'; data: { message: Record<string, unknown>; info: MessageInfo } }
  | { type: 'receipt'; data: Receipt }
  | { type: 'undecryptable_message'; data: UndecryptableMessage }
  | { type: 'notification'; data: Record<string, unknown> }
  | { type: 'chat_presence'; data: ChatPresenceUpdate }
  | { type: 'presence'; data: PresenceUpdate }
  | { type: 'picture_update'; data: PictureUpdate }
  | { type: 'user_about_update'; data: UserAboutUpdate }
  | { type: 'contact_updated'; data: ContactUpdated }
  | { type: 'contact_number_changed'; data: ContactNumberChanged }
  | { type: 'contact_sync_requested'; data: ContactSyncRequested }
  | { type: 'joined_group'; data: Record<string, unknown> }
  | { type: 'group_update'; data: GroupUpdate }
  | { type: 'contact_update'; data: ContactUpdate }
  | { type: 'push_name_update'; data: PushNameUpdate }
  | { type: 'self_push_name_updated'; data: SelfPushNameUpdated }
  | { type: 'pin_update'; data: PinUpdate }
  | { type: 'mute_update'; data: MuteUpdate }
  | { type: 'archive_update'; data: ArchiveUpdate }
  | { type: 'star_update'; data: StarUpdate }
  | { type: 'mark_chat_as_read_update'; data: MarkChatAsReadUpdate }
  | { type: 'history_sync'; data: Record<string, unknown> }
  | { type: 'offline_sync_preview'; data: OfflineSyncPreview }
  | { type: 'offline_sync_completed'; data: OfflineSyncCompleted }
  | { type: 'device_list_update'; data: DeviceListUpdate }
  | { type: 'business_status_update'; data: BusinessStatusUpdate }
  | { type: 'stream_replaced'; data: Record<string, never> }
  | { type: 'temporary_ban'; data: TemporaryBan }
  | { type: 'connect_failure'; data: ConnectFailure }
  | { type: 'stream_error'; data: StreamError }
  | { type: 'disappearing_mode_changed'; data: DisappearingModeChanged }
  | { type: 'newsletter_live_update'; data: NewsletterLiveUpdate }
  | { type: 'qr_scanned_without_multidevice'; data: Record<string, never> }
  | { type: 'client_outdated'; data: Record<string, never> };

export interface WhatsAppClientConfig {
  transport: JsTransportCallbacks;
  httpClient: JsHttpClientConfig;
  onEvent?: (event: WhatsAppEvent) => void;
}

/** Initialize the WASM engine. Call once before creating clients. */
export function initWasmEngine(): void;

/**
 * Create a full WhatsApp client running in WASM.
 *
 * @param transport_config WebSocket transport callbacks (connect/send/disconnect)
 * @param http_config HTTP client callbacks (execute via fetch)
 * @param on_event Optional event callback — receives typed WhatsApp events in order
 */
export function createWhatsAppClient(
  transport_config: JsTransportCallbacks,
  http_config: JsHttpClientConfig,
  on_event?: ((event: WhatsAppEvent) => void) | null,
): Promise<WasmWhatsAppClient>;
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
#[wasm_bindgen(js_name = initWasmEngine, skip_typescript)]
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
#[wasm_bindgen(js_name = createWhatsAppClient, skip_typescript)]
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

    // ── Message management ──────────────────────────────────────────────

    /// Edit a previously sent message.
    #[wasm_bindgen(js_name = editMessage, skip_typescript)]
    pub async fn edit_message(
        &self,
        jid: &str,
        message_id: &str,
        new_content: JsValue,
    ) -> Result<JsValue, JsValue> {
        let to: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;
        let msg: waproto::whatsapp::Message = serde_wasm_bindgen::from_value(new_content)
            .map_err(|e| JsValue::from_str(&format!("invalid message: {e}")))?;

        let new_id = self
            .client
            .edit_message(to, message_id, msg)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        Ok(JsValue::from_str(&new_id))
    }

    /// Revoke (delete) a sent message.
    #[wasm_bindgen(js_name = revokeMessage, skip_typescript)]
    pub async fn revoke_message(
        &self,
        jid: &str,
        message_id: &str,
        participant: Option<String>,
    ) -> Result<(), JsValue> {
        let to: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let revoke_type = match participant {
            Some(p) => {
                let sender: Jid = p.parse().map_err(|e: wacore_binary::jid::JidError| {
                    JsValue::from_str(&format!("invalid participant jid: {e}"))
                })?;
                whatsapp_rust::RevokeType::Admin {
                    original_sender: sender,
                }
            }
            None => whatsapp_rust::RevokeType::Sender,
        };

        self.client
            .revoke_message(to, message_id, revoke_type)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
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

    /// Update a group's subject (name).
    #[wasm_bindgen(js_name = groupUpdateSubject, skip_typescript)]
    pub async fn group_update_subject(&self, jid: &str, subject: &str) -> Result<(), JsValue> {
        let group_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let group_subject = whatsapp_rust::features::GroupSubject::new(subject)
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        self.client
            .groups()
            .set_subject(&group_jid, group_subject)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    /// Update a group's description. Pass null/undefined to remove.
    #[wasm_bindgen(js_name = groupUpdateDescription, skip_typescript)]
    pub async fn group_update_description(
        &self,
        jid: &str,
        description: Option<String>,
    ) -> Result<(), JsValue> {
        let group_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let desc = match description {
            Some(d) => Some(
                whatsapp_rust::features::GroupDescription::new(&d)
                    .map_err(|e| JsValue::from_str(&format!("{e}")))?,
            ),
            None => None,
        };

        self.client
            .groups()
            .set_description(&group_jid, desc, None)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    /// Leave a group.
    #[wasm_bindgen(js_name = groupLeave, skip_typescript)]
    pub async fn group_leave(&self, jid: &str) -> Result<(), JsValue> {
        let group_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        self.client
            .groups()
            .leave(&group_jid)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    /// Update group participants (add, remove, promote, demote).
    #[wasm_bindgen(js_name = groupParticipantsUpdate, skip_typescript)]
    pub async fn group_participants_update(
        &self,
        jid: &str,
        participants: Vec<String>,
        action: &str,
    ) -> Result<JsValue, JsValue> {
        let group_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let participant_jids: Vec<Jid> = participants
            .iter()
            .map(|p| {
                p.parse()
                    .unwrap_or_else(|_| Jid::new(p, wacore_binary::jid::DEFAULT_USER_SERVER))
            })
            .collect();

        match action {
            "add" => {
                let result = self
                    .client
                    .groups()
                    .add_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("{e}")))?;
                participant_change_to_js(&result)
            }
            "remove" => {
                let result = self
                    .client
                    .groups()
                    .remove_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("{e}")))?;
                participant_change_to_js(&result)
            }
            "promote" => {
                self.client
                    .groups()
                    .promote_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("{e}")))?;
                Ok(JsValue::UNDEFINED)
            }
            "demote" => {
                self.client
                    .groups()
                    .demote_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("{e}")))?;
                Ok(JsValue::UNDEFINED)
            }
            _ => Err(JsValue::from_str(
                "action must be 'add', 'remove', 'promote', or 'demote'",
            )),
        }
    }

    /// Fetch all groups the user is participating in.
    #[wasm_bindgen(js_name = groupFetchAllParticipating, skip_typescript)]
    pub async fn group_fetch_all_participating(&self) -> Result<JsValue, JsValue> {
        let groups = self
            .client
            .groups()
            .get_participating()
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        // Convert HashMap<String, GroupMetadata> to a JS object
        let obj = js_sys::Object::new();
        for (key, metadata) in &groups {
            let js_metadata = group_metadata_to_js(metadata)?;
            js_sys::Reflect::set(&obj, &JsValue::from_str(key), &js_metadata)?;
        }
        Ok(obj.into())
    }

    /// Get the invite link for a group.
    #[wasm_bindgen(js_name = groupInviteCode, skip_typescript)]
    pub async fn group_invite_code(&self, jid: &str) -> Result<JsValue, JsValue> {
        let group_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let link = self
            .client
            .groups()
            .get_invite_link(&group_jid, false)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        Ok(JsValue::from_str(&link))
    }

    /// Update a group setting (locked, announce, membership_approval).
    #[wasm_bindgen(js_name = groupSettingUpdate, skip_typescript)]
    pub async fn group_setting_update(
        &self,
        jid: &str,
        setting: &str,
        value: bool,
    ) -> Result<(), JsValue> {
        let group_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        match setting {
            "locked" => self
                .client
                .groups()
                .set_locked(&group_jid, value)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?,
            "announce" => self
                .client
                .groups()
                .set_announce(&group_jid, value)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?,
            "membership_approval" => {
                let mode = if value {
                    whatsapp_rust::MembershipApprovalMode::On
                } else {
                    whatsapp_rust::MembershipApprovalMode::Off
                };
                self.client
                    .groups()
                    .set_membership_approval(&group_jid, mode)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("{e}")))?;
            }
            _ => {
                return Err(JsValue::from_str(
                    "setting must be 'locked', 'announce', or 'membership_approval'",
                ));
            }
        }

        Ok(())
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

    /// Get the profile picture URL for a user or group.
    ///
    /// `picture_type` should be "preview" or "image".
    #[wasm_bindgen(js_name = profilePictureUrl, skip_typescript)]
    pub async fn profile_picture_url(
        &self,
        jid: &str,
        picture_type: &str,
    ) -> Result<JsValue, JsValue> {
        let target: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let preview = match picture_type {
            "preview" => true,
            "image" => false,
            _ => {
                return Err(JsValue::from_str(
                    "picture_type must be 'preview' or 'image'",
                ));
            }
        };

        let result = self
            .client
            .contacts()
            .get_profile_picture(&target, preview)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        match result {
            Some(pic) => {
                let obj = js_sys::Object::new();
                js_sys::Reflect::set(&obj, &"id".into(), &pic.id.as_str().into())?;
                js_sys::Reflect::set(&obj, &"url".into(), &pic.url.as_str().into())?;
                set_optional_str(&obj, "directPath", &pic.direct_path)?;
                set_optional_str(&obj, "hash", &pic.hash)?;
                Ok(obj.into())
            }
            None => Ok(JsValue::NULL),
        }
    }

    /// Fetch user info for one or more JIDs.
    #[wasm_bindgen(js_name = fetchUserInfo, skip_typescript)]
    pub async fn fetch_user_info(&self, jids: Vec<String>) -> Result<JsValue, JsValue> {
        let parsed_jids: Vec<Jid> = jids
            .iter()
            .map(|j| {
                j.parse::<Jid>()
                    .map_err(|e| JsValue::from_str(&format!("invalid jid '{j}': {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let result = self
            .client
            .contacts()
            .get_user_info(&parsed_jids)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        let obj = js_sys::Object::new();
        for (jid, info) in &result {
            let info_obj = js_sys::Object::new();
            js_sys::Reflect::set(&info_obj, &"jid".into(), &info.jid.to_string().into())?;
            js_sys::Reflect::set(
                &info_obj,
                &"lid".into(),
                &match &info.lid {
                    Some(l) => JsValue::from_str(&l.to_string()),
                    None => JsValue::NULL,
                },
            )?;
            set_optional_str(&info_obj, "status", &info.status)?;
            set_optional_str(&info_obj, "pictureId", &info.picture_id)?;
            js_sys::Reflect::set(&info_obj, &"isBusiness".into(), &info.is_business.into())?;
            js_sys::Reflect::set(&obj, &JsValue::from_str(&jid.to_string()), &info_obj)?;
        }
        Ok(obj.into())
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

    /// Set the profile picture for the logged-in user.
    #[wasm_bindgen(js_name = updateProfilePicture, skip_typescript)]
    pub async fn update_profile_picture(&self, img_data: Vec<u8>) -> Result<JsValue, JsValue> {
        let result = self
            .client
            .profile()
            .set_profile_picture(img_data)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"id".into(), &result.id.as_str().into())?;
        Ok(obj.into())
    }

    /// Remove the profile picture for the logged-in user.
    #[wasm_bindgen(js_name = removeProfilePicture, skip_typescript)]
    pub async fn remove_profile_picture(&self) -> Result<JsValue, JsValue> {
        let result = self
            .client
            .profile()
            .remove_profile_picture()
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"id".into(), &result.id.as_str().into())?;
        Ok(obj.into())
    }

    /// Update the user's status text (about).
    #[wasm_bindgen(js_name = updateProfileStatus, skip_typescript)]
    pub async fn update_profile_status(&self, status: &str) -> Result<(), JsValue> {
        self.client
            .profile()
            .set_status_text(status)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    // ── Blocking ──────────────────────────────────────────────────────────

    /// Block or unblock a contact.
    ///
    /// `action` must be "block" or "unblock".
    #[wasm_bindgen(js_name = updateBlockStatus, skip_typescript)]
    pub async fn update_block_status(&self, jid: &str, action: &str) -> Result<(), JsValue> {
        let target: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        match action {
            "block" => self
                .client
                .blocking()
                .block(&target)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?,
            "unblock" => self
                .client
                .blocking()
                .unblock(&target)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?,
            _ => {
                return Err(JsValue::from_str("action must be 'block' or 'unblock'"));
            }
        }

        Ok(())
    }

    /// Fetch the full blocklist.
    #[wasm_bindgen(js_name = fetchBlocklist, skip_typescript)]
    pub async fn fetch_blocklist(&self) -> Result<JsValue, JsValue> {
        let entries = self
            .client
            .blocking()
            .get_blocklist()
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        let arr = js_sys::Array::new();
        for entry in &entries {
            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&obj, &"jid".into(), &entry.jid.to_string().into())?;
            set_optional_num(&obj, "timestamp", &entry.timestamp.map(|v| v as f64))?;
            arr.push(&obj.into());
        }
        Ok(arr.into())
    }

    // ── Chat actions ──────────────────────────────────────────────────────

    /// Pin or unpin a chat.
    #[wasm_bindgen(js_name = pinChat, skip_typescript)]
    pub async fn pin_chat(&self, jid: &str, pin: bool) -> Result<(), JsValue> {
        let chat_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        if pin {
            self.client
                .chat_actions()
                .pin_chat(&chat_jid)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        } else {
            self.client
                .chat_actions()
                .unpin_chat(&chat_jid)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        }

        Ok(())
    }

    /// Mute or unmute a chat.
    ///
    /// Pass a positive timestamp (ms) to mute until that time, or null/undefined to unmute.
    #[wasm_bindgen(js_name = muteChat, skip_typescript)]
    pub async fn mute_chat(&self, jid: &str, mute_until: Option<f64>) -> Result<(), JsValue> {
        let chat_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        match mute_until {
            Some(ts) => self
                .client
                .chat_actions()
                .mute_chat_until(&chat_jid, ts as i64)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?,
            None => self
                .client
                .chat_actions()
                .unmute_chat(&chat_jid)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?,
        }

        Ok(())
    }

    /// Archive or unarchive a chat.
    #[wasm_bindgen(js_name = archiveChat, skip_typescript)]
    pub async fn archive_chat(&self, jid: &str, archive: bool) -> Result<(), JsValue> {
        let chat_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        if archive {
            self.client
                .chat_actions()
                .archive_chat(&chat_jid)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        } else {
            self.client
                .chat_actions()
                .unarchive_chat(&chat_jid)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        }

        Ok(())
    }

    /// Star or unstar a message.
    #[wasm_bindgen(js_name = starMessage, skip_typescript)]
    pub async fn star_message(
        &self,
        jid: &str,
        message_id: &str,
        star: bool,
    ) -> Result<(), JsValue> {
        let chat_jid: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        if star {
            self.client
                .chat_actions()
                .star_message(&chat_jid, None, message_id, true)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        } else {
            self.client
                .chat_actions()
                .unstar_message(&chat_jid, None, message_id, true)
                .await
                .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        }

        Ok(())
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

    /// Subscribe to a contact's presence updates.
    #[wasm_bindgen(js_name = presenceSubscribe, skip_typescript)]
    pub async fn presence_subscribe(&self, jid: &str) -> Result<(), JsValue> {
        let target: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        self.client
            .presence()
            .subscribe(&target)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))
    }

    // ── Newsletter ────────────────────────────────────────────────────────

    /// Create a new newsletter (channel).
    #[wasm_bindgen(js_name = newsletterCreate, skip_typescript)]
    pub async fn newsletter_create(
        &self,
        name: &str,
        description: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let result = self
            .client
            .newsletter()
            .create(name, description.as_deref())
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        newsletter_metadata_to_js(&result)
    }

    /// Fetch metadata for a newsletter by JID.
    #[wasm_bindgen(js_name = newsletterMetadata, skip_typescript)]
    pub async fn newsletter_metadata(&self, jid: &str) -> Result<JsValue, JsValue> {
        let target: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let result = self
            .client
            .newsletter()
            .get_metadata(&target)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        newsletter_metadata_to_js(&result)
    }

    /// Subscribe (join) a newsletter.
    #[wasm_bindgen(js_name = newsletterSubscribe, skip_typescript)]
    pub async fn newsletter_subscribe(&self, jid: &str) -> Result<JsValue, JsValue> {
        let target: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        let result = self
            .client
            .newsletter()
            .join(&target)
            .await
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;

        newsletter_metadata_to_js(&result)
    }

    /// Unsubscribe (leave) a newsletter.
    #[wasm_bindgen(js_name = newsletterUnsubscribe, skip_typescript)]
    pub async fn newsletter_unsubscribe(&self, jid: &str) -> Result<(), JsValue> {
        let target: Jid = jid
            .parse()
            .map_err(|e: wacore_binary::jid::JidError| JsValue::from_str(&format!("{e}")))?;

        self.client
            .newsletter()
            .leave(&target)
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

/// Convert Vec<ParticipantChangeResponse> to a JS array.
fn participant_change_to_js(
    responses: &[whatsapp_rust::features::ParticipantChangeResponse],
) -> Result<JsValue, JsValue> {
    let arr = js_sys::Array::new();
    for r in responses {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"jid".into(), &r.jid.to_string().into())?;
        set_optional_str(&obj, "status", &r.status)?;
        set_optional_str(&obj, "error", &r.error)?;
        arr.push(&obj.into());
    }
    Ok(arr.into())
}

/// Convert NewsletterMetadata to a JS object.
fn newsletter_metadata_to_js(
    meta: &whatsapp_rust::features::NewsletterMetadata,
) -> Result<JsValue, JsValue> {
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"jid".into(), &meta.jid.to_string().into())?;
    js_sys::Reflect::set(&obj, &"name".into(), &meta.name.as_str().into())?;
    set_optional_str(&obj, "description", &meta.description)?;
    js_sys::Reflect::set(
        &obj,
        &"subscriberCount".into(),
        &(meta.subscriber_count as f64).into(),
    )?;
    js_sys::Reflect::set(
        &obj,
        &"verification".into(),
        &format!("{:?}", meta.verification).into(),
    )?;
    js_sys::Reflect::set(&obj, &"state".into(), &format!("{:?}", meta.state).into())?;
    set_optional_str(&obj, "pictureUrl", &meta.picture_url)?;
    set_optional_str(&obj, "previewUrl", &meta.preview_url)?;
    set_optional_str(&obj, "inviteCode", &meta.invite_code)?;
    js_sys::Reflect::set(
        &obj,
        &"role".into(),
        &match &meta.role {
            Some(r) => JsValue::from_str(&format!("{:?}", r)),
            None => JsValue::NULL,
        },
    )?;
    set_optional_num(&obj, "creationTime", &meta.creation_time.map(|v| v as f64))?;
    Ok(obj.into())
}
