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

/** JS storage callbacks for persistent backend. */
export interface JsStoreCallbacks {
  get(store: string, key: string): Promise<Uint8Array | null>;
  set(store: string, key: string, value: Uint8Array): Promise<void>;
  delete(store: string, key: string): Promise<void>;
}

/** Initialize the WASM engine. Call once before creating clients. */
export function initWasmEngine(): void;

/**
 * Create a full WhatsApp client running in WASM.
 *
 * @param transport_config WebSocket transport callbacks (connect/send/disconnect)
 * @param http_config HTTP client callbacks (execute via fetch)
 * @param on_event Optional event callback — receives typed WhatsApp events in order
 * @param store Optional JS storage callbacks — if provided, enables persistent storage
 */
export function createWhatsAppClient(
  transport_config: JsTransportCallbacks,
  http_config: JsHttpClientConfig,
  on_event?: ((event: WhatsAppEvent) => void) | null,
  store?: JsStoreCallbacks | null,
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
        let (event_tx, event_rx) = async_channel::bounded(16384);

        // Single consumer loop — guarantees event ordering.
        // Yields to the event loop every 50 events to prevent starvation
        // during large offline message batches.
        wasm_bindgen_futures::spawn_local(async move {
            let mut count = 0u32;
            while let Ok(event) = event_rx.recv().await {
                if let Err(e) = callback.call1(&JsValue::NULL, &event) {
                    log::warn!("JS event callback threw: {:?}", e);
                }
                count += 1;
                if count.is_multiple_of(50) {
                    // Macrotask yield — lets I/O callbacks (WebSocket, storage) run
                    crate::runtime::set_timeout_0().await;
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
                &"business_name".into(),
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
                &"business_name".into(),
                &pe.business_name.as_str().into(),
            )?;
            js_sys::Reflect::set(&d, &"platform".into(), &pe.platform.as_str().into())?;
            js_sys::Reflect::set(&d, &"error".into(), &pe.error.as_str().into())?;
            ("pair_error", d.into())
        }
        Event::LoggedOut(lo) => {
            let d = js_sys::Object::new();
            js_sys::Reflect::set(&d, &"on_connect".into(), &lo.on_connect.into())?;
            js_sys::Reflect::set(&d, &"reason".into(), &format!("{:?}", lo.reason).into())?;
            ("logged_out", d.into())
        }
        Event::Message(msg, info) => {
            let d = js_sys::Object::new();
            // Proto message → camelCase (matches protobufjs/Baileys convention)
            js_sys::Reflect::set(
                &d,
                &"message".into(),
                &crate::camel_serializer::to_js_value_camel(msg.as_ref())?,
            )?;
            // MessageInfo → snake_case (wacore type, Option B)
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
/// Accepts an optional JS logger (pino-compatible) to route all Rust logs through.
/// If no logger is provided, falls back to console.log with "warn" level.
#[wasm_bindgen(js_name = initWasmEngine, skip_typescript)]
pub fn init_wasm_engine(logger: JsValue) {
    console_error_panic_hook::set_once();

    if !logger.is_undefined() && !logger.is_null() {
        // Use the JS logger adapter — all Rust log::* calls go through pino
        let js_logger: crate::logger::JsLogger = logger.unchecked_into();
        let _ = crate::logger::set_logger(js_logger);
    } else {
        // No logger provided — fall back to console.log
        let _ = console_log::init_with_level(log::Level::Warn);
    }

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
    store: Option<JsValue>,
    cache_config_js: Option<JsValue>,
) -> Result<WasmWhatsAppClient, JsValue> {
    let runtime = Arc::new(WasmRuntime) as Arc<dyn wacore::runtime::Runtime>;
    let backend = match store {
        Some(ref store_val) if !store_val.is_null() && !store_val.is_undefined() => {
            let get_fn = js_sys::Reflect::get(store_val, &"get".into())
                .map_err(|_| JsValue::from_str("store.get is required"))?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| JsValue::from_str("store.get must be a function"))?;
            let set_fn = js_sys::Reflect::get(store_val, &"set".into())
                .map_err(|_| JsValue::from_str("store.set is required"))?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| JsValue::from_str("store.set must be a function"))?;
            let delete_fn = js_sys::Reflect::get(store_val, &"delete".into())
                .map_err(|_| JsValue::from_str("store.delete is required"))?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| JsValue::from_str("store.delete must be a function"))?;
            info!("Using JS-backed persistent storage");
            js_backend::new_js_backend(get_fn, set_fn, delete_fn)
        }
        _ => {
            info!("Using in-memory storage (no persistence)");
            js_backend::new_in_memory_backend()
        }
    };
    let transport_factory = Arc::new(JsTransportFactory::from_js(transport_config)?)
        as Arc<dyn wacore::net::TransportFactory>;
    let http_client =
        Arc::new(JsHttpClientAdapter::from_js(http_config)?) as Arc<dyn wacore::net::HttpClient>;

    let persistence_manager: Arc<whatsapp_rust::store::persistence_manager::PersistenceManager> =
        Arc::new(
            whatsapp_rust::store::persistence_manager::PersistenceManager::new(backend.clone())
                .await
                .map_err(|e| JsValue::from_str(&format!("create persistence manager: {e}")))?,
        );

    // Start background saver — flushes dirty Device state to the backend
    persistence_manager
        .clone()
        .run_background_saver(runtime.clone(), std::time::Duration::from_secs(5));

    let cache_config = build_cache_config(cache_config_js.as_ref())?;

    let (client, sync_rx) = whatsapp_rust::Client::new_with_cache_config(
        runtime.clone(),
        persistence_manager.clone(),
        transport_factory,
        http_client,
        Some(DEFAULT_WA_WEB_VERSION),
        cache_config,
    )
    .await;

    if let Some(callback) = on_event {
        let handler = Arc::new(JsEventHandler::new(callback)) as Arc<dyn EventHandler>;
        client.register_handler(handler);
    }

    Ok(WasmWhatsAppClient {
        client,
        runtime,
        sync_rx: Some(sync_rx),
        persistence_manager,
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
    persistence_manager: Arc<whatsapp_rust::store::persistence_manager::PersistenceManager>,
}

#[wasm_bindgen]
impl WasmWhatsAppClient {
    // ── Connection ───────────────────────────────────────────────────────

    /// Start the main client loop in the background.
    ///
    /// Spawns the connection loop (connect, handshake, message loop, reconnect)
    /// as a background task and returns immediately. The loop runs until `disconnect()`
    /// is called.
    ///
    /// Not `async` to avoid holding a wasm-bindgen borrow on `self` that would
    /// prevent calling other methods (disconnect, etc.).
    pub fn run(&mut self) -> Result<(), JsValue> {
        if self.sync_rx.is_none() {
            return Err(JsValue::from_str("run() has already been called"));
        }
        let client = self.client.clone();
        let runtime = self.runtime.clone();
        let sync_rx = self.sync_rx.take();

        // Start sync worker — processes history sync and app state sync tasks.
        // Must drain promptly to prevent the sync channel (capacity 32) from
        // blocking the message processing loop.
        if let Some(receiver) = sync_rx {
            let worker_client = client.clone();
            runtime
                .spawn(Box::pin(async move {
                    while let Ok(task) = receiver.recv().await {
                        worker_client.process_sync_task(task).await;
                    }
                    info!("Sync worker shutting down.");
                }))
                .detach();
        }

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
        self.client.connect().await.map_err(js_err)
    }

    /// Disconnect the client and flush pending state to storage.
    pub async fn disconnect(&self) {
        self.client.disconnect().await;
        if let Err(e) = self.persistence_manager.flush().await {
            log::warn!("Failed to flush state on disconnect: {e}");
        }
    }

    /// Enable or disable automatic reconnection on disconnect.
    /// Enabled by default. When disabled, the client will not attempt
    /// to reconnect after an unexpected disconnection.
    #[wasm_bindgen(js_name = setAutoReconnect)]
    pub fn set_auto_reconnect(&self, enabled: bool) {
        self.client
            .enable_auto_reconnect
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
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

    // ── Device props ─────────────────────────────────────────────────────

    /// Set device properties (OS name, browser/platform type).
    /// This controls what device name is shown on the phone (e.g. "Chrome", "Firefox").
    ///
    /// `os` — OS name (e.g. "Mac OS", "Windows", "Ubuntu")
    /// `browser` — Browser name mapped to PlatformType (e.g. "Chrome", "Firefox", "Safari")
    #[wasm_bindgen(js_name = setDeviceProps)]
    pub async fn set_device_props(&self, os: &str, browser: &str) {
        use wacore::store::commands::DeviceCommand;
        use waproto::whatsapp::device_props;

        let platform_type = match browser {
            "Chrome" => device_props::PlatformType::Chrome,
            "Firefox" => device_props::PlatformType::Firefox,
            "Safari" => device_props::PlatformType::Safari,
            "Edge" => device_props::PlatformType::Edge,
            "Opera" => device_props::PlatformType::Opera,
            "Desktop" => device_props::PlatformType::Desktop,
            _ => device_props::PlatformType::Chrome,
        };

        self.persistence_manager
            .process_command(DeviceCommand::SetDeviceProps(
                Some(os.to_string()),
                None,
                Some(platform_type),
            ))
            .await;
    }

    /// Override the WhatsApp Web version used for the connection.
    /// Accepts [major, minor, patch] array.
    #[wasm_bindgen(js_name = setVersion)]
    pub fn set_version(&self, major: u32, minor: u32, patch: u32) {
        use wacore::store::commands::DeviceCommand;
        // This sets the app version on the device, which is sent during login.
        // The actual protocol version is set once at client creation and can't
        // be changed after, but this ensures subsequent reconnections use it.
        let pm = self.persistence_manager.clone();
        let rt = self.runtime.clone();
        rt.spawn(Box::pin(async move {
            pm.process_command(DeviceCommand::SetAppVersion((major, minor, patch)))
                .await;
        }))
        .detach();
    }

    // ── Pairing ──────────────────────────────────────────────────────────

    /// Request a pairing code for phone number login (alternative to QR).
    ///
    /// Returns the 8-character pairing code to enter on the phone.
    #[wasm_bindgen(js_name = requestPairingCode)]
    pub async fn request_pairing_code(
        &self,
        phone_number: &str,
        custom_code: Option<String>,
    ) -> Result<JsValue, JsValue> {
        use whatsapp_rust::pair_code::PairCodeOptions;
        let options = PairCodeOptions {
            phone_number: phone_number.to_string(),
            custom_code,
            ..Default::default()
        };
        let code = self.client.pair_with_code(options).await.map_err(js_err)?;
        Ok(JsValue::from_str(&code))
    }

    // ── Sending messages ─────────────────────────────────────────────────

    /// Send an E2E encrypted message from a JS object.
    #[wasm_bindgen(js_name = sendMessage)]
    pub async fn send_message(&self, jid: &str, message: JsValue) -> Result<JsValue, JsValue> {
        let (to, msg) = parse_jid_and_msg(jid, message)?;
        let id = self.client.send_message(to, msg).await.map_err(js_err)?;
        Ok(JsValue::from_str(&id))
    }

    /// Send a message from protobuf binary bytes.
    #[wasm_bindgen(js_name = sendMessageBytes)]
    pub async fn send_message_bytes(&self, jid: &str, bytes: &[u8]) -> Result<JsValue, JsValue> {
        let (to, msg) = parse_jid_and_msg_bytes(jid, bytes)?;
        let id = self.client.send_message(to, msg).await.map_err(js_err)?;
        Ok(JsValue::from_str(&id))
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
        let (to, msg) = parse_jid_and_msg(jid, new_content)?;
        let id = self
            .client
            .edit_message(to, message_id, msg)
            .await
            .map_err(js_err)?;
        Ok(JsValue::from_str(&id))
    }

    /// Edit a previously sent message from protobuf bytes.
    #[wasm_bindgen(js_name = editMessageBytes)]
    pub async fn edit_message_bytes(
        &self,
        jid: &str,
        message_id: &str,
        bytes: &[u8],
    ) -> Result<String, JsValue> {
        let (to, msg) = parse_jid_and_msg_bytes(jid, bytes)?;
        self.client
            .edit_message(to, message_id, msg)
            .await
            .map_err(js_err)
    }

    /// Revoke (delete) a sent message.
    #[wasm_bindgen(js_name = revokeMessage)]
    pub async fn revoke_message(
        &self,
        jid: &str,
        message_id: &str,
        participant: Option<String>,
    ) -> Result<(), JsValue> {
        let to = parse_jid(jid)?;

        let revoke_type = match participant {
            Some(p) => {
                let sender = parse_jid(&p)?;
                whatsapp_rust::RevokeType::Admin {
                    original_sender: sender,
                }
            }
            None => whatsapp_rust::RevokeType::Sender,
        };

        self.client
            .revoke_message(to, message_id, revoke_type)
            .await
            .map_err(js_err)
    }

    // ── Groups ───────────────────────────────────────────────────────────

    /// Get metadata for a group.
    #[wasm_bindgen(js_name = getGroupMetadata)]
    pub async fn group_metadata(&self, jid: &str) -> Result<JsValue, JsValue> {
        let group_jid = parse_jid(jid)?;

        let metadata = self
            .client
            .groups()
            .get_metadata(&group_jid)
            .await
            .map_err(js_err)?;

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
            .map_err(js_err)?;

        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"gid".into(), &result.gid.to_string().into())?;
        Ok(obj.into())
    }

    /// Update a group's subject (name).
    #[wasm_bindgen(js_name = groupUpdateSubject)]
    pub async fn group_update_subject(&self, jid: &str, subject: &str) -> Result<(), JsValue> {
        let group_jid = parse_jid(jid)?;

        let group_subject = whatsapp_rust::features::GroupSubject::new(subject).map_err(js_err)?;

        self.client
            .groups()
            .set_subject(&group_jid, group_subject)
            .await
            .map_err(js_err)
    }

    /// Update a group's description. Pass null/undefined to remove.
    #[wasm_bindgen(js_name = groupUpdateDescription)]
    pub async fn group_update_description(
        &self,
        jid: &str,
        description: Option<String>,
    ) -> Result<(), JsValue> {
        let group_jid = parse_jid(jid)?;

        let desc = description
            .as_deref()
            .map(whatsapp_rust::features::GroupDescription::new)
            .transpose()
            .map_err(js_err)?;

        self.client
            .groups()
            .set_description(&group_jid, desc, None)
            .await
            .map_err(js_err)
    }

    /// Leave a group.
    #[wasm_bindgen(js_name = groupLeave)]
    pub async fn group_leave(&self, jid: &str) -> Result<(), JsValue> {
        let group_jid = parse_jid(jid)?;

        self.client.groups().leave(&group_jid).await.map_err(js_err)
    }

    /// Update group participants (add, remove, promote, demote).
    #[wasm_bindgen(js_name = groupParticipantsUpdate, skip_typescript)]
    pub async fn group_participants_update(
        &self,
        jid: &str,
        participants: Vec<String>,
        action: &str,
    ) -> Result<JsValue, JsValue> {
        let group_jid = parse_jid(jid)?;

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
                    .map_err(js_err)?;
                participant_change_to_js(&result)
            }
            "remove" => {
                let result = self
                    .client
                    .groups()
                    .remove_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(js_err)?;
                participant_change_to_js(&result)
            }
            "promote" => {
                self.client
                    .groups()
                    .promote_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(js_err)?;
                Ok(JsValue::UNDEFINED)
            }
            "demote" => {
                self.client
                    .groups()
                    .demote_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(js_err)?;
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
            .map_err(js_err)?;

        let obj = js_sys::Object::new();
        for (key, metadata) in &groups {
            let js_metadata = group_metadata_to_js(metadata)?;
            js_sys::Reflect::set(&obj, &JsValue::from_str(key), &js_metadata)?;
        }
        Ok(obj.into())
    }

    /// Get the invite link for a group.
    #[wasm_bindgen(js_name = groupInviteCode)]
    pub async fn group_invite_code(&self, jid: &str) -> Result<String, JsValue> {
        let group_jid = parse_jid(jid)?;

        self.client
            .groups()
            .get_invite_link(&group_jid, false)
            .await
            .map_err(js_err)
    }

    /// Update a group setting (locked, announce, membership_approval).
    #[wasm_bindgen(js_name = groupSettingUpdate)]
    pub async fn group_setting_update(
        &self,
        jid: &str,
        setting: &str,
        value: bool,
    ) -> Result<(), JsValue> {
        let group_jid = parse_jid(jid)?;

        match setting {
            "locked" => self
                .client
                .groups()
                .set_locked(&group_jid, value)
                .await
                .map_err(js_err)?,
            "announce" => self
                .client
                .groups()
                .set_announce(&group_jid, value)
                .await
                .map_err(js_err)?,
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
                    .map_err(js_err)?;
            }
            _ => {
                return Err(JsValue::from_str(
                    "setting must be 'locked', 'announce', or 'membership_approval'",
                ));
            }
        }

        Ok(())
    }

    /// Set disappearing messages timer for a group (0 to disable).
    #[wasm_bindgen(js_name = groupToggleEphemeral)]
    pub async fn group_toggle_ephemeral(&self, jid: &str, expiration: u32) -> Result<(), JsValue> {
        let group_jid = parse_jid(jid)?;
        self.client
            .groups()
            .set_ephemeral(&group_jid, expiration)
            .await
            .map_err(js_err)
    }

    /// Revoke a group's invite link (generates new one).
    #[wasm_bindgen(js_name = groupRevokeInvite)]
    pub async fn group_revoke_invite(&self, jid: &str) -> Result<String, JsValue> {
        let group_jid = parse_jid(jid)?;
        let new_code = self
            .client
            .groups()
            .get_invite_link(&group_jid, true)
            .await
            .map_err(js_err)?;
        Ok(new_code)
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
            .map_err(js_err)?;

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
    #[wasm_bindgen(js_name = profilePictureUrl)]
    pub async fn profile_picture_url(
        &self,
        jid: &str,
        picture_type: &str,
    ) -> Result<Option<crate::result_types::ProfilePictureInfo>, JsValue> {
        let target = parse_jid(jid)?;
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
            .map_err(js_err)?;

        Ok(result.map(|pic| crate::result_types::ProfilePictureInfo {
            id: pic.id.clone(),
            url: pic.url.clone(),
            direct_path: pic.direct_path.clone(),
            hash: pic.hash.clone(),
        }))
    }

    /// Fetch user info for one or more JIDs.
    #[wasm_bindgen(js_name = fetchUserInfo, skip_typescript)]
    pub async fn fetch_user_info(&self, jids: Vec<String>) -> Result<JsValue, JsValue> {
        let parsed_jids: Vec<Jid> = jids
            .iter()
            .map(|j| parse_jid(j))
            .collect::<Result<Vec<_>, _>>()?;

        let result = self
            .client
            .contacts()
            .get_user_info(&parsed_jids)
            .await
            .map_err(js_err)?;

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
            .map_err(js_err)
    }

    /// Set the profile picture for the logged-in user.
    #[wasm_bindgen(js_name = updateProfilePicture)]
    pub async fn update_profile_picture(
        &self,
        img_data: Vec<u8>,
    ) -> Result<crate::result_types::ProfilePictureResult, JsValue> {
        let result = self
            .client
            .profile()
            .set_profile_picture(img_data)
            .await
            .map_err(js_err)?;

        Ok(crate::result_types::ProfilePictureResult {
            id: result.id.clone(),
        })
    }

    /// Remove the profile picture for the logged-in user.
    #[wasm_bindgen(js_name = removeProfilePicture)]
    pub async fn remove_profile_picture(
        &self,
    ) -> Result<crate::result_types::ProfilePictureResult, JsValue> {
        let result = self
            .client
            .profile()
            .remove_profile_picture()
            .await
            .map_err(js_err)?;

        Ok(crate::result_types::ProfilePictureResult {
            id: result.id.clone(),
        })
    }

    /// Update the user's status text (about).
    #[wasm_bindgen(js_name = updateProfileStatus)]
    pub async fn update_profile_status(&self, status: &str) -> Result<(), JsValue> {
        self.client
            .profile()
            .set_status_text(status)
            .await
            .map_err(js_err)
    }

    // ── Blocking ──────────────────────────────────────────────────────────

    /// Block or unblock a contact.
    ///
    /// `action` must be "block" or "unblock".
    #[wasm_bindgen(js_name = updateBlockStatus)]
    pub async fn update_block_status(&self, jid: &str, action: &str) -> Result<(), JsValue> {
        let target = parse_jid(jid)?;

        match action {
            "block" => self
                .client
                .blocking()
                .block(&target)
                .await
                .map_err(js_err)?,
            "unblock" => self
                .client
                .blocking()
                .unblock(&target)
                .await
                .map_err(js_err)?,
            _ => {
                return Err(JsValue::from_str("action must be 'block' or 'unblock'"));
            }
        }

        Ok(())
    }

    /// Fetch the full blocklist.
    #[wasm_bindgen(js_name = fetchBlocklist)]
    pub async fn fetch_blocklist(
        &self,
    ) -> Result<Vec<crate::result_types::BlocklistEntryResult>, JsValue> {
        let entries = self
            .client
            .blocking()
            .get_blocklist()
            .await
            .map_err(js_err)?;

        Ok(entries
            .iter()
            .map(|e| crate::result_types::BlocklistEntryResult {
                jid: e.jid.to_string(),
                timestamp: e.timestamp.map(|v| v as f64),
            })
            .collect())
    }

    // ── Chat actions ──────────────────────────────────────────────────────

    /// Pin or unpin a chat.
    #[wasm_bindgen(js_name = pinChat)]
    pub async fn pin_chat(&self, jid: &str, pin: bool) -> Result<(), JsValue> {
        let chat_jid = parse_jid(jid)?;

        if pin {
            self.client.chat_actions().pin_chat(&chat_jid).await
        } else {
            self.client.chat_actions().unpin_chat(&chat_jid).await
        }
        .map_err(js_err)
    }

    /// Mute or unmute a chat.
    ///
    /// Pass a positive timestamp (ms) to mute until that time, or null/undefined to unmute.
    #[wasm_bindgen(js_name = muteChat)]
    pub async fn mute_chat(&self, jid: &str, mute_until: Option<f64>) -> Result<(), JsValue> {
        let chat_jid = parse_jid(jid)?;

        match mute_until {
            Some(ts) => {
                self.client
                    .chat_actions()
                    .mute_chat_until(&chat_jid, ts as i64)
                    .await
            }
            None => self.client.chat_actions().unmute_chat(&chat_jid).await,
        }
        .map_err(js_err)
    }

    /// Archive or unarchive a chat.
    #[wasm_bindgen(js_name = archiveChat)]
    pub async fn archive_chat(&self, jid: &str, archive: bool) -> Result<(), JsValue> {
        let chat_jid = parse_jid(jid)?;

        if archive {
            self.client
                .chat_actions()
                .archive_chat(&chat_jid, None)
                .await
        } else {
            self.client
                .chat_actions()
                .unarchive_chat(&chat_jid, None)
                .await
        }
        .map_err(js_err)
    }

    /// Star or unstar a message.
    #[wasm_bindgen(js_name = starMessage)]
    pub async fn star_message(
        &self,
        jid: &str,
        message_id: &str,
        star: bool,
    ) -> Result<(), JsValue> {
        let chat_jid = parse_jid(jid)?;

        if star {
            self.client
                .chat_actions()
                .star_message(&chat_jid, None, message_id, true)
                .await
        } else {
            self.client
                .chat_actions()
                .unstar_message(&chat_jid, None, message_id, true)
                .await
        }
        .map_err(js_err)
    }

    /// Mark a chat as read or unread via app state mutation.
    /// Different from readMessages (which sends read receipts).
    #[wasm_bindgen(js_name = markChatAsRead)]
    pub async fn mark_chat_as_read(&self, jid: &str, read: bool) -> Result<(), JsValue> {
        let chat_jid = parse_jid(jid)?;
        self.client
            .chat_actions()
            .mark_chat_as_read(&chat_jid, read, None)
            .await
            .map_err(js_err)
    }

    /// Delete a chat via app state mutation.
    #[wasm_bindgen(js_name = deleteChat)]
    pub async fn delete_chat(&self, jid: &str) -> Result<(), JsValue> {
        let chat_jid = parse_jid(jid)?;
        self.client
            .chat_actions()
            .delete_chat(&chat_jid, true, None)
            .await
            .map_err(js_err)
    }

    /// Delete a message for self (not for everyone).
    #[wasm_bindgen(js_name = deleteMessageForMe)]
    pub async fn delete_message_for_me(
        &self,
        jid: &str,
        message_id: &str,
        from_me: bool,
    ) -> Result<(), JsValue> {
        let chat_jid = parse_jid(jid)?;
        self.client
            .chat_actions()
            .delete_message_for_me(&chat_jid, None, message_id, from_me, true, None)
            .await
            .map_err(js_err)
    }

    // ── Polls ─────────────────────────────────────────────────────────

    /// Create and send a poll. Returns `{ messageId, messageSecret }`.
    ///
    /// The `messageSecret` (32 bytes) is needed to decrypt votes later.
    #[wasm_bindgen(js_name = createPoll)]
    pub async fn create_poll(
        &self,
        jid: &str,
        name: &str,
        options: Vec<String>,
        selectable_count: u32,
    ) -> Result<JsValue, JsValue> {
        let to = parse_jid(jid)?;
        let (msg_id, message_secret) = self
            .client
            .polls()
            .create(&to, name, &options, selectable_count)
            .await
            .map_err(js_err)?;
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"messageId".into(), &msg_id.into())?;
        js_sys::Reflect::set(
            &obj,
            &"messageSecret".into(),
            &js_sys::Uint8Array::from(&message_secret[..]).into(),
        )?;
        Ok(obj.into())
    }

    /// Vote on a poll. Returns message ID.
    #[wasm_bindgen(js_name = votePoll)]
    pub async fn vote_poll(
        &self,
        chat_jid: &str,
        poll_msg_id: &str,
        poll_creator_jid: &str,
        message_secret: &[u8],
        option_names: Vec<String>,
    ) -> Result<JsValue, JsValue> {
        let chat = parse_jid(chat_jid)?;
        let creator = parse_jid(poll_creator_jid)?;
        let id = self
            .client
            .polls()
            .vote(&chat, poll_msg_id, &creator, message_secret, &option_names)
            .await
            .map_err(js_err)?;
        Ok(JsValue::from_str(&id))
    }

    /// Send a status/story message to specified recipients.
    #[wasm_bindgen(js_name = sendStatusMessage)]
    pub async fn send_status_message(
        &self,
        message: JsValue,
        recipients: Vec<String>,
    ) -> Result<JsValue, JsValue> {
        let msg: waproto::whatsapp::Message = {
            let snake = crate::proto::to_snake_case_js(&message);
            serde_wasm_bindgen::from_value(snake)
                .map_err(|e| JsValue::from_str(&format!("invalid message: {e}")))?
        };
        let jids: Vec<Jid> = recipients
            .iter()
            .map(|s| parse_jid(s))
            .collect::<Result<_, _>>()?;
        let id = self
            .client
            .status()
            .send_raw(msg, jids, Default::default())
            .await
            .map_err(js_err)?;
        Ok(JsValue::from_str(&id))
    }

    // ── Read receipts ─────────────────────────────────────────────────

    /// Mark messages as read by sending read receipts.
    ///
    /// `keys` is an array of `{ remoteJid, id, participant? }` objects.
    #[wasm_bindgen(js_name = readMessages)]
    pub async fn read_messages(&self, keys: JsValue) -> Result<(), JsValue> {
        let arr = js_sys::Array::from(&keys);

        use std::collections::HashMap;
        let mut grouped: HashMap<(String, Option<String>), Vec<String>> = HashMap::new();

        for i in 0..arr.length() {
            let key = arr.get(i);
            let remote_jid = js_sys::Reflect::get(&key, &"remoteJid".into())
                .ok()
                .and_then(|v| v.as_string())
                .ok_or_else(|| JsValue::from_str("key.remoteJid is required"))?;
            let id = js_sys::Reflect::get(&key, &"id".into())
                .ok()
                .and_then(|v| v.as_string())
                .ok_or_else(|| JsValue::from_str("key.id is required"))?;
            let participant = js_sys::Reflect::get(&key, &"participant".into())
                .ok()
                .and_then(|v| v.as_string());

            grouped
                .entry((remote_jid, participant))
                .or_default()
                .push(id);
        }

        for ((chat_jid_str, participant_str), ids) in grouped {
            let chat_jid = parse_jid(&chat_jid_str)?;
            let participant_jid = participant_str.as_deref().map(parse_jid).transpose()?;

            self.client
                .mark_as_read(&chat_jid, participant_jid.as_ref(), ids)
                .await
                .map_err(js_err)?;
        }

        Ok(())
    }

    // ── Group invite ────────────────────────────────────────────────────

    /// Join a group using an invite code.
    #[wasm_bindgen(js_name = groupAcceptInvite)]
    pub async fn group_accept_invite(&self, code: &str) -> Result<JsValue, JsValue> {
        let jid = self
            .client
            .groups()
            .join_with_invite_code(code)
            .await
            .map_err(js_err)?;
        Ok(JsValue::from_str(&jid.group_jid().to_string()))
    }

    /// Join a group via a GroupInviteMessage (V4 invite).
    #[wasm_bindgen(js_name = groupAcceptInviteV4)]
    pub async fn group_accept_invite_v4(
        &self,
        group_jid: &str,
        code: &str,
        expiration: f64,
        admin_jid: &str,
    ) -> Result<JsValue, JsValue> {
        let group = parse_jid(group_jid)?;
        let admin = parse_jid(admin_jid)?;
        let result = self
            .client
            .groups()
            .join_with_invite_v4(&group, code, expiration as i64, &admin)
            .await
            .map_err(js_err)?;
        Ok(JsValue::from_str(&result.group_jid().to_string()))
    }

    /// Get group info from an invite code (without joining).
    /// Returns the same shape as groupMetadata.
    #[wasm_bindgen(js_name = groupGetInviteInfo)]
    pub async fn group_get_invite_info(&self, code: &str) -> Result<JsValue, JsValue> {
        let metadata = self
            .client
            .groups()
            .get_invite_info(code)
            .await
            .map_err(js_err)?;
        group_metadata_to_js(&metadata)
    }

    /// Get list of pending join requests for a group.
    #[wasm_bindgen(js_name = groupRequestParticipantsList)]
    pub async fn group_request_participants_list(&self, jid: &str) -> Result<JsValue, JsValue> {
        let group_jid = parse_jid(jid)?;
        let list = self
            .client
            .groups()
            .get_membership_requests(&group_jid)
            .await
            .map_err(js_err)?;
        // MembershipRequest derives Serialize
        serde_wasm_bindgen::to_value(&list).map_err(js_err)
    }

    /// Approve or reject pending join requests.
    #[wasm_bindgen(js_name = groupRequestParticipantsUpdate)]
    pub async fn group_request_participants_update(
        &self,
        jid: &str,
        participants: Vec<String>,
        action: &str,
    ) -> Result<JsValue, JsValue> {
        let group_jid = parse_jid(jid)?;
        let participant_jids: Vec<Jid> = participants
            .iter()
            .map(|s| parse_jid(s))
            .collect::<Result<Vec<_>, _>>()?;

        let result = match action {
            "approve" => {
                self.client
                    .groups()
                    .approve_membership_requests(&group_jid, &participant_jids)
                    .await
            }
            "reject" => {
                self.client
                    .groups()
                    .reject_membership_requests(&group_jid, &participant_jids)
                    .await
            }
            _ => return Err(JsValue::from_str("action must be 'approve' or 'reject'")),
        };

        result.map_err(js_err)?;
        Ok(JsValue::undefined())
    }

    // ── Privacy settings ──────────────────────────────────────────────

    /// Fetch all privacy settings.
    #[wasm_bindgen(js_name = fetchPrivacySettings)]
    pub async fn fetch_privacy_settings(&self) -> Result<JsValue, JsValue> {
        let response = self.client.fetch_privacy_settings().await.map_err(js_err)?;
        let obj = js_sys::Object::new();
        for setting in &response.settings {
            js_sys::Reflect::set(
                &obj,
                &setting.category.as_str().into(),
                &setting.value.as_str().into(),
            )?;
        }
        Ok(obj.into())
    }

    /// Update a single privacy setting.
    #[wasm_bindgen(js_name = updatePrivacySetting)]
    pub async fn update_privacy_setting(&self, category: &str, value: &str) -> Result<(), JsValue> {
        self.client
            .set_privacy_setting(category, value)
            .await
            .map_err(js_err)
    }

    /// Set default disappearing messages duration (seconds). 0 to disable.
    #[wasm_bindgen(js_name = updateDefaultDisappearingMode)]
    pub async fn update_default_disappearing_mode(&self, duration: u32) -> Result<(), JsValue> {
        self.client
            .set_default_disappearing_mode(duration)
            .await
            .map_err(js_err)
    }

    // ── Calls ────────────────────────────────────────────────────────────

    /// Reject an incoming call.
    #[wasm_bindgen(js_name = rejectCall)]
    pub async fn reject_call(&self, call_id: &str, call_from: &str) -> Result<(), JsValue> {
        let from_jid = parse_jid(call_from)?;
        self.client
            .reject_call(call_id, &from_jid)
            .await
            .map_err(js_err)
    }

    // ── User status ──────────────────────────────────────────────────────

    /// Fetch user status/about text for one or more JIDs.
    #[wasm_bindgen(js_name = fetchStatus)]
    pub async fn fetch_status(&self, jids: Vec<String>) -> Result<JsValue, JsValue> {
        let jid_refs: Vec<&str> = jids.iter().map(|s| s.as_str()).collect();
        let infos = self
            .client
            .contacts()
            .get_info(&jid_refs)
            .await
            .map_err(js_err)?;
        let arr = js_sys::Array::new();
        for info in infos {
            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&obj, &"jid".into(), &info.jid.to_string().into())?;
            if let Some(status) = &info.status {
                js_sys::Reflect::set(&obj, &"status".into(), &status.into())?;
            }
            arr.push(&obj.into());
        }
        Ok(arr.into())
    }

    // ── Business profile ───────────────────────────────────────────────

    /// Get business profile information for a JID.
    #[wasm_bindgen(js_name = getBusinessProfile)]
    pub async fn get_business_profile(&self, jid: &str) -> Result<JsValue, JsValue> {
        let target_jid = parse_jid(jid)?;
        let profile = self
            .client
            .execute(wacore::iq::business::BusinessProfileSpec::new(&target_jid))
            .await
            .map_err(js_err)?;
        match profile {
            Some(p) => serde_wasm_bindgen::to_value(&p).map_err(js_err),
            None => Ok(JsValue::undefined()),
        }
    }

    // ── Message history ──────────────────────────────────────────────────

    /// Request on-demand message history from the primary phone.
    /// Returns the message ID of the PDO request.
    /// Results will arrive as history_sync events.
    #[wasm_bindgen(js_name = fetchMessageHistory)]
    pub async fn fetch_message_history(
        &self,
        count: i32,
        chat_jid: &str,
        oldest_msg_id: &str,
        oldest_msg_from_me: bool,
        oldest_msg_timestamp_ms: f64,
    ) -> Result<JsValue, JsValue> {
        let chat = parse_jid(chat_jid)?;
        let msg_id = self
            .client
            .fetch_message_history(
                &chat,
                oldest_msg_id,
                oldest_msg_from_me,
                oldest_msg_timestamp_ms as i64,
                count,
            )
            .await
            .map_err(js_err)?;
        Ok(JsValue::from_str(&msg_id))
    }

    // ── Group member add mode ────────────────────────────────────────────

    /// Set who can add members to a group: "admin_add" or "all_member_add".
    #[wasm_bindgen(js_name = groupMemberAddMode)]
    pub async fn group_member_add_mode(&self, jid: &str, mode: &str) -> Result<(), JsValue> {
        let group_jid = parse_jid(jid)?;
        let add_mode = match mode {
            "admin_add" => whatsapp_rust::features::MemberAddMode::AdminAdd,
            "all_member_add" => whatsapp_rust::features::MemberAddMode::AllMemberAdd,
            _ => {
                return Err(JsValue::from_str(
                    "mode must be 'admin_add' or 'all_member_add'",
                ));
            }
        };
        self.client
            .groups()
            .set_member_add_mode(&group_jid, add_mode)
            .await
            .map_err(js_err)
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
            .map_err(js_err)
    }

    /// Subscribe to a contact's presence updates.
    #[wasm_bindgen(js_name = presenceSubscribe)]
    pub async fn presence_subscribe(&self, jid: &str) -> Result<(), JsValue> {
        let target = parse_jid(jid)?;

        self.client
            .presence()
            .subscribe(&target)
            .await
            .map_err(js_err)
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
            .map_err(js_err)?;

        newsletter_metadata_to_js(&result)
    }

    /// Fetch metadata for a newsletter by JID.
    #[wasm_bindgen(js_name = newsletterMetadata, skip_typescript)]
    pub async fn newsletter_metadata(&self, jid: &str) -> Result<JsValue, JsValue> {
        let target = parse_jid(jid)?;

        let result = self
            .client
            .newsletter()
            .get_metadata(&target)
            .await
            .map_err(js_err)?;

        newsletter_metadata_to_js(&result)
    }

    /// Subscribe (join) a newsletter.
    #[wasm_bindgen(js_name = newsletterSubscribe, skip_typescript)]
    pub async fn newsletter_subscribe(&self, jid: &str) -> Result<JsValue, JsValue> {
        let target = parse_jid(jid)?;

        let result = self
            .client
            .newsletter()
            .join(&target)
            .await
            .map_err(js_err)?;

        newsletter_metadata_to_js(&result)
    }

    /// Unsubscribe (leave) a newsletter.
    #[wasm_bindgen(js_name = newsletterUnsubscribe)]
    pub async fn newsletter_unsubscribe(&self, jid: &str) -> Result<(), JsValue> {
        let target = parse_jid(jid)?;

        self.client
            .newsletter()
            .leave(&target)
            .await
            .map_err(js_err)
    }

    // ── Media reupload ────────────────────────────────────────────────────

    /// Request the server to re-upload expired media.
    ///
    /// Returns the new `directPath` on success.
    /// Throws on failure (not found, decryption error, timeout, etc.).
    #[wasm_bindgen(js_name = requestMediaReupload, skip_typescript)]
    pub async fn request_media_reupload(
        &self,
        msg_id: &str,
        chat_jid: &str,
        media_key: &[u8],
        is_from_me: bool,
        participant: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let chat = parse_jid(chat_jid)?;

        let participant_jid = participant.as_deref().map(parse_jid).transpose()?;

        let req = whatsapp_rust::MediaReuploadRequest {
            msg_id,
            chat_jid: &chat,
            media_key,
            is_from_me,
            participant: participant_jid.as_ref(),
        };

        let result = self
            .client
            .media_reupload()
            .request(&req)
            .await
            .map_err(js_err)?;

        match result {
            whatsapp_rust::MediaRetryResult::Success { direct_path } => {
                Ok(JsValue::from_str(&direct_path))
            }
            whatsapp_rust::MediaRetryResult::NotFound => {
                Err(JsValue::from_str("Media not found on server"))
            }
            whatsapp_rust::MediaRetryResult::DecryptionError => {
                Err(JsValue::from_str("Media decryption error"))
            }
            whatsapp_rust::MediaRetryResult::GeneralError => {
                Err(JsValue::from_str("Media reupload failed"))
            }
        }
    }

    // ── Chat state ───────────────────────────────────────────────────────

    /// Send a chat state update (typing indicator).
    ///
    /// `state` must be one of: "composing", "recording", "paused".
    #[wasm_bindgen(js_name = sendChatState)]
    pub async fn send_chat_state(&self, jid: &str, state: &str) -> Result<(), JsValue> {
        let to = parse_jid(jid)?;

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
            .map_err(js_err)
    }

    // ── Media ────────────────────────────────────────────────────────────

    /// Get media connection info (auth token + upload hosts).
    ///
    /// Returns `{ auth: string, ttl: number, hosts: [{hostname: string, maxContentLengthBytes: number}] }`.
    #[wasm_bindgen(js_name = getMediaConn)]
    pub async fn get_media_conn(
        &self,
        force: bool,
    ) -> Result<crate::result_types::MediaConnResult, JsValue> {
        let conn = self
            .client
            .refresh_media_conn(force)
            .await
            .map_err(js_err)?;

        Ok(crate::result_types::MediaConnResult {
            auth: conn.auth.clone(),
            ttl: conn.ttl as f64,
            hosts: conn
                .hosts
                .iter()
                .map(|h| crate::result_types::MediaHost {
                    hostname: h.hostname.clone(),
                })
                .collect(),
        })
    }

    /// Download and decrypt media from raw parameters.
    ///
    /// Handles CDN failover, auth refresh, HMAC-SHA256 verification, and
    /// AES-256-CBC decryption internally. Returns decrypted media bytes.
    #[wasm_bindgen(js_name = downloadMedia)]
    pub async fn download_media(
        &self,
        direct_path: &str,
        media_key: &[u8],
        file_sha256: &[u8],
        file_enc_sha256: &[u8],
        file_length: f64,
        media_type: &str,
    ) -> Result<js_sys::Uint8Array, JsValue> {
        let mt = parse_media_type(media_type)?;
        let data = self
            .client
            .download_from_params(
                direct_path,
                media_key,
                file_sha256,
                file_enc_sha256,
                file_length as u64,
                mt,
            )
            .await
            .map_err(js_err)?;
        Ok(js_sys::Uint8Array::from(&data[..]))
    }

    /// Download, decrypt, and return a Web ReadableStream of decrypted chunks.
    ///
    /// Same as `downloadMedia` but returns a `ReadableStream` instead of buffering
    /// the entire file. In Node.js, consume with `Readable.fromWeb(stream)`.
    #[wasm_bindgen(js_name = downloadMediaStream)]
    pub fn download_media_stream(
        &self,
        direct_path: &str,
        media_key: &[u8],
        file_sha256: &[u8],
        file_enc_sha256: &[u8],
        file_length: f64,
        media_type: &str,
    ) -> Result<web_sys::ReadableStream, JsValue> {
        let mt = parse_media_type(media_type)?;
        let client = self.client.clone();
        let direct_path = direct_path.to_string();
        let media_key = media_key.to_vec();
        let file_sha256 = file_sha256.to_vec();
        let file_enc_sha256 = file_enc_sha256.to_vec();
        let file_length = file_length as u64;

        // Channel with backpressure (capacity 2 keeps memory bounded)
        let (mut tx, rx) = futures::channel::mpsc::channel::<Result<JsValue, JsValue>>(2);

        wasm_bindgen_futures::spawn_local(async move {
            use futures::SinkExt;

            match client
                .download_from_params(
                    &direct_path,
                    &media_key,
                    &file_sha256,
                    &file_enc_sha256,
                    file_length,
                    mt,
                )
                .await
            {
                Ok(data) => {
                    // Stream in 64KB chunks to avoid holding the full buffer in JS
                    const CHUNK_SIZE: usize = 65536;
                    for chunk in data.chunks(CHUNK_SIZE) {
                        let js_chunk = js_sys::Uint8Array::from(chunk);
                        if tx.send(Ok(js_chunk.into())).await.is_err() {
                            break; // Consumer cancelled the stream
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(JsValue::from_str(&e.to_string()))).await;
                }
            }
            // tx dropped here → stream ends
        });

        let readable = wasm_streams::ReadableStream::from_stream(rx);
        Ok(readable.into_raw())
    }

    // ── Upload ────────────────────────────────────────────────────────────

    /// Upload media: encrypt in memory + upload with CDN failover and retry.
    ///
    /// Takes raw plaintext bytes. Handles AES-256-CBC encryption, HMAC-SHA256
    /// signing, multi-host CDN upload, auth refresh, and resumable upload (≥5MB).
    #[wasm_bindgen(js_name = uploadMedia)]
    pub async fn upload_media(
        &self,
        data: &[u8],
        media_type: &str,
    ) -> Result<crate::result_types::UploadMediaResult, JsValue> {
        let mt = parse_media_type(media_type)?;
        let resp = self
            .client
            .upload(data.to_vec(), mt)
            .await
            .map_err(js_err)?;
        Ok(crate::result_types::UploadMediaResult {
            url: resp.url,
            direct_path: resp.direct_path,
            media_key: resp.media_key,
            file_sha256: resp.file_sha256,
            file_enc_sha256: resp.file_enc_sha256,
            file_length: resp.file_length as f64,
        })
    }

    /// Streaming encrypt: read plaintext from input stream, encrypt with
    /// True streaming encrypt via `MediaEncryptor`: processes plaintext chunk-by-chunk
    /// from JS ReadableStream, encrypts with AES-256-CBC, writes ciphertext to JS WritableStream.
    ///
    /// Peak memory: ~130KB (copy buffer + flush buffer + crypto state).
    #[wasm_bindgen(js_name = encryptMediaStream)]
    pub async fn encrypt_media_stream(
        &self,
        input: web_sys::ReadableStream,
        output: web_sys::WritableStream,
        media_type: &str,
    ) -> Result<crate::result_types::EncryptMediaResult, JsValue> {
        use futures::SinkExt;
        use futures::StreamExt;
        use wacore::upload::MediaEncryptor;

        let mt = parse_media_type(media_type)?;

        let rs = wasm_streams::ReadableStream::from_raw(input);
        let mut reader = rs.into_stream();
        let ws = wasm_streams::WritableStream::from_raw(output);
        let mut writer = ws.into_sink();

        const FLUSH_THRESHOLD: usize = 65536;

        let mut enc = MediaEncryptor::new(mt).map_err(js_err)?;
        let mut out_buf = Vec::with_capacity(FLUSH_THRESHOLD + 16);
        let mut copy_buf = vec![0u8; FLUSH_THRESHOLD];

        while let Some(chunk_result) = reader.next().await {
            let chunk =
                chunk_result.map_err(|e| JsValue::from_str(&format!("read error: {e:?}")))?;
            let arr = js_sys::Uint8Array::new(&chunk);
            let len = arr.length() as usize;
            if len == 0 {
                continue;
            }

            if len > copy_buf.len() {
                copy_buf.resize(len, 0);
            }
            arr.copy_to(&mut copy_buf[..len]);

            enc.update(&copy_buf[..len], &mut out_buf);

            if out_buf.len() >= FLUSH_THRESHOLD {
                let js_chunk = js_sys::Uint8Array::from(out_buf.as_slice());
                writer
                    .send(js_chunk.into())
                    .await
                    .map_err(|e| JsValue::from_str(&format!("write error: {e:?}")))?;
                out_buf.clear();
            }
        }

        let info = enc.finalize(&mut out_buf).map_err(js_err)?;

        if !out_buf.is_empty() {
            let js_chunk = js_sys::Uint8Array::from(out_buf.as_slice());
            writer
                .send(js_chunk.into())
                .await
                .map_err(|e| JsValue::from_str(&format!("write error: {e:?}")))?;
        }
        writer
            .close()
            .await
            .map_err(|e| JsValue::from_str(&format!("close error: {e:?}")))?;

        Ok(crate::result_types::EncryptMediaResult {
            media_key: info.media_key.to_vec(),
            file_sha256: info.file_sha256.to_vec(),
            file_enc_sha256: info.file_enc_sha256.to_vec(),
            file_length: info.file_length as f64,
        })
    }

    /// Upload pre-encrypted media with streaming body.
    ///
    /// `get_body` is a JS function `() => ReadableStream<Uint8Array>` — called
    /// for each upload attempt (retry creates a fresh stream).
    /// Handles CDN failover, auth refresh, and resumable upload (≥5MB).
    #[wasm_bindgen(js_name = uploadEncryptedMediaStream)]
    pub async fn upload_encrypted_media_stream(
        &self,
        get_body: &js_sys::Function,
        media_key: &[u8],
        file_sha256: &[u8],
        file_enc_sha256: &[u8],
        file_length: f64,
        media_type: &str,
    ) -> Result<crate::result_types::UploadMediaResult, JsValue> {
        let mt = parse_media_type(media_type)?;
        let file_length = file_length as u64;
        let token = base64_url_encode(file_enc_sha256);
        let mms_type = mt.mms_type();

        let mut force_refresh = false;

        for attempt in 0..=1u32 {
            let media_conn = self
                .client
                .refresh_media_conn(force_refresh)
                .await
                .map_err(js_err)?;

            let mut retry_auth = false;

            for host in &media_conn.hosts {
                // Resumable check for large files (≥5MB)
                if file_length >= 5 * 1024 * 1024 {
                    let check_url = format!(
                        "https://{}/mms/{}/{}?auth={}&token={}&resume=1",
                        host.hostname, mms_type, token, media_conn.auth, token
                    );
                    let check_req = wacore::net::HttpRequest::post(check_url)
                        .with_header("Origin", "https://web.whatsapp.com");
                    if let Ok(resp) = self.client.http_client.execute(check_req).await
                        && resp.status_code < 400
                        && let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&resp.body)
                        && parsed.get("resume").and_then(|v| v.as_str()) == Some("complete")
                        && let (Some(url), Some(dp)) = (
                            parsed.get("url").and_then(|v| v.as_str()),
                            parsed.get("direct_path").and_then(|v| v.as_str()),
                        )
                    {
                        return Ok(crate::result_types::UploadMediaResult {
                            url: url.to_string(),
                            direct_path: dp.to_string(),
                            media_key: media_key.to_vec(),
                            file_sha256: file_sha256.to_vec(),
                            file_enc_sha256: file_enc_sha256.to_vec(),
                            file_length: file_length as f64,
                        });
                    }
                }

                let upload_url = format!(
                    "https://{}/mms/{}/{}?auth={}&token={}",
                    host.hostname, mms_type, token, media_conn.auth, token
                );

                // Get fresh ReadableStream from factory
                let body_stream = get_body
                    .call0(&JsValue::NULL)
                    .map_err(|e| JsValue::from_str(&format!("getBody() failed: {e:?}")))?;

                // Try streaming upload via JS HTTP client
                let result = stream_upload_via_js(&self.client, &upload_url, body_stream).await;

                match result {
                    Ok(resp) if resp.status_code < 400 => {
                        let parsed: serde_json::Value =
                            serde_json::from_slice(&resp.body).map_err(js_err)?;
                        let url = parsed
                            .get("url")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| JsValue::from_str("missing url in response"))?;
                        let dp = parsed
                            .get("direct_path")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| JsValue::from_str("missing direct_path in response"))?;
                        return Ok(crate::result_types::UploadMediaResult {
                            url: url.to_string(),
                            direct_path: dp.to_string(),
                            media_key: media_key.to_vec(),
                            file_sha256: file_sha256.to_vec(),
                            file_enc_sha256: file_enc_sha256.to_vec(),
                            file_length: file_length as f64,
                        });
                    }
                    Ok(resp) if is_auth_error(resp.status_code) && attempt == 0 => {
                        force_refresh = true;
                        retry_auth = true;
                        break;
                    }
                    Ok(resp) => {
                        log::warn!(
                            "Upload to {} failed with status {}",
                            host.hostname,
                            resp.status_code
                        );
                    }
                    Err(e) => {
                        log::warn!("Upload to {} failed: {:?}", host.hostname, e);
                    }
                }
            }

            if !retry_auth {
                break;
            }
        }

        Err(JsValue::from_str("Upload failed on all hosts"))
    }

    // ── State getters ────────────────────────────────────────────────────

    /// Get the current push name.
    #[wasm_bindgen(js_name = getPushName)]
    pub async fn get_push_name(&self) -> String {
        self.client.get_push_name().await
    }

    /// Get the own JID (phone number JID) if logged in.
    ///
    /// Returns the non-AD JID (without device suffix), e.g. "559980000014@s.whatsapp.net".
    /// This is the JID used for addressing in messages.
    #[wasm_bindgen(js_name = getJid)]
    pub async fn get_jid(&self) -> Option<String> {
        self.client
            .get_pn()
            .await
            .map(|j| j.to_non_ad().to_string())
    }

    /// Get the own LID (linked identity) if available.
    ///
    /// Returns the non-AD LID (without device suffix), e.g. "100000012345678@lid".
    #[wasm_bindgen(js_name = getLid)]
    pub async fn get_lid(&self) -> Option<String> {
        self.client
            .get_lid()
            .await
            .map(|j| j.to_non_ad().to_string())
    }

    /// Returns a snapshot of internal memory diagnostics (cache sizes, session counts, etc.).
    #[wasm_bindgen(js_name = getMemoryDiagnostics)]
    pub async fn get_memory_diagnostics(&self) -> JsValue {
        let d = self.client.memory_diagnostics().await;
        let obj = js_sys::Object::new();
        let set = |k: &str, v: f64| {
            let _ = js_sys::Reflect::set(&obj, &k.into(), &v.into());
        };
        set("groupCache", d.group_cache as f64);
        set("deviceCache", d.device_cache as f64);
        set("deviceRegistryCache", d.device_registry_cache as f64);
        set("lidPnLidEntries", d.lid_pn_lid_entries as f64);
        set("lidPnPnEntries", d.lid_pn_pn_entries as f64);
        set("retriedGroupMessages", d.retried_group_messages as f64);
        set("recentMessages", d.recent_messages as f64);
        set("messageRetryCounts", d.message_retry_counts as f64);
        set("pdoPendingRequests", d.pdo_pending_requests as f64);
        set("sessionLocks", d.session_locks as f64);
        set("messageQueues", d.message_queues as f64);
        set("messageEnqueueLocks", d.message_enqueue_locks as f64);
        set("responseWaiters", d.response_waiters as f64);
        set("nodeWaiters", d.node_waiters as f64);
        set("pendingRetries", d.pending_retries as f64);
        set("presenceSubscriptions", d.presence_subscriptions as f64);
        set("appStateKeyRequests", d.app_state_key_requests as f64);
        set("appStateSyncing", d.app_state_syncing as f64);
        set("signalCacheSessions", d.signal_cache_sessions as f64);
        set("signalCacheIdentities", d.signal_cache_identities as f64);
        set("signalCacheSenderKeys", d.signal_cache_sender_keys as f64);
        set("chatstateHandlers", d.chatstate_handlers as f64);
        obj.into()
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
        &crate::proto::to_js_value(&metadata.addressing_mode)?,
    )?;

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

// ---------------------------------------------------------------------------
// Poll vote decryption — standalone functions (not on WasmWhatsAppClient)
// ---------------------------------------------------------------------------

/// Decrypt a poll vote. Returns selected option names as a string array.
#[wasm_bindgen(js_name = decryptPollVote)]
pub fn decrypt_poll_vote(
    enc_payload: &[u8],
    enc_iv: &[u8],
    message_secret: &[u8],
    poll_msg_id: &str,
    poll_creator_jid: &str,
    voter_jid: &str,
    option_names: Vec<String>,
) -> Result<JsValue, JsValue> {
    let creator = parse_jid(poll_creator_jid)?;
    let voter = parse_jid(voter_jid)?;

    let selected_hashes = whatsapp_rust::features::Polls::decrypt_vote(
        enc_payload,
        enc_iv,
        message_secret,
        poll_msg_id,
        &creator,
        &voter,
    )
    .map_err(js_err)?;

    // Map hashes back to option names
    let option_map: Vec<([u8; 32], &str)> = option_names
        .iter()
        .map(|n| (wacore::poll::compute_option_hash(n), n.as_str()))
        .collect();

    let arr = js_sys::Array::new();
    for hash in &selected_hashes {
        if let Ok(hash_arr) = <[u8; 32]>::try_from(hash.as_slice())
            && let Some((_, name)) = option_map.iter().find(|(h, _)| *h == hash_arr)
        {
            arr.push(&JsValue::from_str(name));
        }
    }
    Ok(arr.into())
}

/// Aggregate all votes for a poll. Returns `[{ name: string, voters: string[] }]`.
#[wasm_bindgen(js_name = getAggregateVotesInPollMessage)]
pub fn get_aggregate_votes_in_poll_message(
    option_names: Vec<String>,
    voters_json: &str,
    message_secret: &[u8],
    poll_msg_id: &str,
    poll_creator_jid: &str,
) -> Result<JsValue, JsValue> {
    let creator = parse_jid(poll_creator_jid)?;
    let voters: Vec<serde_json::Value> = serde_json::from_str(voters_json).map_err(js_err)?;

    let vote_data: Vec<(Jid, Vec<u8>, Vec<u8>)> = voters
        .iter()
        .filter_map(|v| {
            let voter_str = v.get("voter")?.as_str()?;
            let voter_jid: Jid = voter_str.parse().ok()?;
            let payload: Vec<u8> = v
                .get("encPayload")?
                .as_array()?
                .iter()
                .filter_map(|b| b.as_u64().map(|n| n as u8))
                .collect();
            let iv: Vec<u8> = v
                .get("encIv")?
                .as_array()?
                .iter()
                .filter_map(|b| b.as_u64().map(|n| n as u8))
                .collect();
            Some((voter_jid, payload, iv))
        })
        .collect();

    let votes_refs: Vec<(&Jid, &[u8], &[u8])> = vote_data
        .iter()
        .map(|(v, p, i)| (v, p.as_slice(), i.as_slice()))
        .collect();

    let results = whatsapp_rust::features::Polls::aggregate_votes(
        &option_names,
        &votes_refs,
        message_secret,
        poll_msg_id,
        &creator,
    )
    .map_err(js_err)?;

    let arr = js_sys::Array::new();
    for r in results {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"name".into(), &r.name.into())?;
        let voters_arr = js_sys::Array::new();
        for v in &r.voters {
            voters_arr.push(&JsValue::from_str(v));
        }
        js_sys::Reflect::set(&obj, &"voters".into(), &voters_arr.into())?;
        arr.push(&obj.into());
    }
    Ok(arr.into())
}

/// Convert any error to JsValue string.
fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Parse a JID string, returning a JS error on failure.
fn parse_jid(jid: &str) -> Result<Jid, JsValue> {
    jid.parse().map_err(js_err)
}

/// Parse a media type string (matching Baileys convention) to the Rust enum.
fn parse_media_type(s: &str) -> Result<whatsapp_rust::download::MediaType, JsValue> {
    use whatsapp_rust::download::MediaType;
    match s {
        "image" => Ok(MediaType::Image),
        "video" => Ok(MediaType::Video),
        "audio" => Ok(MediaType::Audio),
        "document" => Ok(MediaType::Document),
        "sticker" => Ok(MediaType::Sticker),
        "thumbnail-link" => Ok(MediaType::LinkThumbnail),
        "md-msg-hist" => Ok(MediaType::History),
        "md-app-state" => Ok(MediaType::AppState),
        _ => Err(JsValue::from_str(&format!("unknown media type: {s}"))),
    }
}

/// Base64-URL-safe (no padding) encoding for upload tokens.
fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// Check if an HTTP status code is a media auth error (401/403).
fn is_auth_error(status: u16) -> bool {
    matches!(status, 401 | 403)
}

/// Execute a streaming upload by calling the JS HTTP client's execute method
/// with the ReadableStream body converted to Uint8Array.
///
/// This buffers the stream body through the existing execute() method.
/// For true streaming, the JS HTTP client should implement executeStreamUpload.
async fn stream_upload_via_js(
    client: &whatsapp_rust::Client,
    url: &str,
    body_stream: JsValue,
) -> Result<wacore::net::HttpResponse, JsValue> {
    // Consume the ReadableStream into bytes
    let rs = wasm_streams::ReadableStream::from_raw(body_stream.unchecked_into());
    let mut stream = rs.into_stream();

    let mut body_bytes = Vec::new();
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| JsValue::from_str(&format!("stream read error: {e:?}")))?;
        let arr = js_sys::Uint8Array::new(&chunk);
        let mut bytes = vec![0u8; arr.length() as usize];
        arr.copy_to(&mut bytes);
        body_bytes.extend_from_slice(&bytes);
    }

    let request = wacore::net::HttpRequest::post(url.to_string())
        .with_header("Content-Type", "application/octet-stream")
        .with_header("Origin", "https://web.whatsapp.com")
        .with_body(body_bytes);

    client.http_client.execute(request).await.map_err(js_err)
}

/// Parse a JID string and deserialize a JS object as a proto Message.
fn parse_jid_and_msg(
    jid: &str,
    message: JsValue,
) -> Result<(Jid, waproto::whatsapp::Message), JsValue> {
    let to = parse_jid(jid)?;
    let snake_message = crate::proto::to_snake_case_js(&message);
    let msg = serde_wasm_bindgen::from_value(snake_message)
        .map_err(|e| JsValue::from_str(&format!("invalid message: {e}")))?;
    Ok((to, msg))
}

fn parse_jid_and_msg_bytes(
    jid: &str,
    bytes: &[u8],
) -> Result<(Jid, waproto::whatsapp::Message), JsValue> {
    use prost::Message;
    let to = parse_jid(jid)?;
    let msg = waproto::whatsapp::Message::decode(bytes)
        .map_err(|e| JsValue::from_str(&format!("invalid message bytes: {e}")))?;
    Ok((to, msg))
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

// ---------------------------------------------------------------------------
// Cache config builder
// ---------------------------------------------------------------------------

/// Build a Rust `CacheConfig` from an optional JS `CacheConfig` object.
/// Omitted fields keep their defaults.
fn build_cache_config(js: Option<&JsValue>) -> Result<whatsapp_rust::CacheConfig, JsValue> {
    use crate::js_cache_store::JsCacheStoreAdapter;
    use std::sync::Arc;

    let mut config = whatsapp_rust::CacheConfig::default();

    let js = match js {
        Some(v) if !v.is_null() && !v.is_undefined() => v,
        _ => return Ok(config),
    };

    // Global store (applied to all pluggable caches unless overridden per-cache)
    let global_store = js_sys::Reflect::get(js, &"store".into())
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
        .and_then(|v| {
            JsCacheStoreAdapter::from_js(&v)
                .ok()
                .map(|a| Arc::new(a) as Arc<dyn whatsapp_rust::CacheStore>)
        });

    if let Some(ref store) = global_store {
        config.cache_stores = whatsapp_rust::CacheStores::all(store.clone());
    }

    // Per-cache overrides
    apply_cache_entry(
        js,
        "group",
        &mut config.group_cache,
        &mut config.cache_stores.group_cache,
        &global_store,
    )?;
    apply_cache_entry(
        js,
        "device",
        &mut config.device_cache,
        &mut config.cache_stores.device_cache,
        &global_store,
    )?;
    apply_cache_entry(
        js,
        "deviceRegistry",
        &mut config.device_registry_cache,
        &mut config.cache_stores.device_registry_cache,
        &global_store,
    )?;
    apply_cache_entry(
        js,
        "lidPn",
        &mut config.lid_pn_cache,
        &mut config.cache_stores.lid_pn_cache,
        &global_store,
    )?;
    apply_cache_entry_simple(
        js,
        "retriedGroupMessages",
        &mut config.retried_group_messages,
    )?;
    apply_cache_entry_simple(js, "recentMessages", &mut config.recent_messages)?;
    apply_cache_entry_simple(js, "messageRetry", &mut config.message_retry_counts)?;

    Ok(config)
}

/// Apply JS overrides to a cache entry that supports custom stores.
fn apply_cache_entry(
    parent: &JsValue,
    key: &str,
    entry: &mut whatsapp_rust::CacheEntryConfig,
    store_slot: &mut Option<std::sync::Arc<dyn whatsapp_rust::CacheStore>>,
    _global_store: &Option<std::sync::Arc<dyn whatsapp_rust::CacheStore>>,
) -> Result<(), JsValue> {
    use crate::js_cache_store::JsCacheStoreAdapter;
    use std::sync::Arc;

    let obj = match js_sys::Reflect::get(parent, &key.into()) {
        Ok(v) if !v.is_undefined() && !v.is_null() => v,
        _ => return Ok(()),
    };

    apply_ttl_capacity(&obj, entry)?;

    // Per-cache store override (takes priority over global)
    if let Ok(store_val) = js_sys::Reflect::get(&obj, &"store".into())
        && !store_val.is_undefined()
        && !store_val.is_null()
        && let Ok(adapter) = JsCacheStoreAdapter::from_js(&store_val)
    {
        *store_slot = Some(Arc::new(adapter) as Arc<dyn whatsapp_rust::CacheStore>);
    }

    Ok(())
}

/// Apply JS overrides to a simple cache entry (no custom store support).
fn apply_cache_entry_simple(
    parent: &JsValue,
    key: &str,
    entry: &mut whatsapp_rust::CacheEntryConfig,
) -> Result<(), JsValue> {
    let obj = match js_sys::Reflect::get(parent, &key.into()) {
        Ok(v) if !v.is_undefined() && !v.is_null() => v,
        _ => return Ok(()),
    };

    apply_ttl_capacity(&obj, entry)
}

/// Shared: apply ttlSecs and capacity from a JS object to a CacheEntryConfig.
fn apply_ttl_capacity(
    obj: &JsValue,
    entry: &mut whatsapp_rust::CacheEntryConfig,
) -> Result<(), JsValue> {
    use std::time::Duration;

    if let Ok(ttl) = js_sys::Reflect::get(obj, &"ttlSecs".into())
        && let Some(secs) = ttl.as_f64()
    {
        entry.timeout = if secs > 0.0 {
            Some(Duration::from_secs(secs as u64))
        } else {
            None
        };
    }

    if let Ok(cap) = js_sys::Reflect::get(obj, &"capacity".into())
        && let Some(c) = cap.as_f64()
    {
        entry.capacity = c as u64;
    }

    Ok(())
}
