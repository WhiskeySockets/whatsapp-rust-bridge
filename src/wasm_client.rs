//! Full WhatsApp client running in WASM.
//!
//! Wraps `whatsapp_rust::Client` with JS-provided adapters for
//! transport (WebSocket), storage (InMemory/JS), and HTTP (fetch).

use std::sync::{Arc, Mutex};

use log::info;
use wacore::types::events::{Event, EventHandler};
use wacore_binary::jid::Jid;
use wasm_bindgen::prelude::*;

use crate::js_backend;
use crate::js_http::JsHttpClientAdapter;
use crate::js_time;
use crate::js_transport::JsTransportFactory;
use crate::js_val_to_error as js_val_err;
use crate::runtime::WasmRuntime;

// ---------------------------------------------------------------------------
// TypeScript type declarations
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Event definition — SINGLE SOURCE OF TRUTH
// ---------------------------------------------------------------------------
//
// `bridge_events!` generates BOTH from one definition:
//   1. The TypeScript `WhatsAppEvent` union type (typescript_custom_section)
//   2. The `event_to_js` Rust dispatch function
//
// To add a new serializable event: add ONE line in `serialize { }`.
// To add a new special event: add in `special { }` AND handle in `event_to_js_special`.

/// Helper: generates one WhatsAppEvent TS union variant line.
macro_rules! ev {
    ($name:literal, $ts_type:literal) => {
        concat!("  | { type: '", $name, "'; data: ", $ts_type, " }\n")
    };
}

macro_rules! bridge_events {
    (
        serialize {
            $( $variant:ident => $name:literal => $ts_type:literal ),* $(,)?
        }
        special {
            $( $xname:literal => $xts:literal ),* $(,)?
        }
    ) => {
        // Generate WhatsAppEvent TypeScript type
        #[wasm_bindgen(typescript_custom_section)]
        const _TS_WHATSAPP_EVENT: &str = concat!(
            "export type WhatsAppEvent =\n",
            $( ev!($name, $ts_type), )*
            $( ev!($xname, $xts), )*
            ";\n",
        );

        // Generate event_to_js dispatch (JS-specific, existing path)
        fn event_to_js(event: &Event) -> Result<JsValue, JsValue> {
            let obj = js_sys::Object::new();
            let (event_type, data) = match event {
                $( Event::$variant(data) => ($name, crate::proto::to_js_value(data)?), )*
                other => return event_to_js_special(other),
            };
            js_sys::Reflect::set(&obj, &"type".into(), &event_type.into())?;
            js_sys::Reflect::set(&obj, &"data".into(), &data)?;
            Ok(obj.into())
        }

        // Generate event_to_json dispatch (host-agnostic)
        /// Serialize an Event to a host-agnostic JSON value: `{"type": "...", "data": {...}}`.
        pub fn event_to_json(event: &Event) -> Result<serde_json::Value, String> {
            let (event_type, data) = match event {
                $( Event::$variant(data) => {
                    let val = serde_json::to_value(data).map_err(|e| e.to_string())?;
                    ($name, val)
                }, )*
                other => return event_to_json_special(other),
            };
            Ok(serde_json::json!({ "type": event_type, "data": data }))
        }
    };
}

bridge_events! {
    serialize {
        // Variant              => "js_name"                       => "TsDataType"
        Receipt                  => "receipt"                       => "Receipt",
        UndecryptableMessage     => "undecryptable_message"         => "UndecryptableMessage",
        ChatPresence             => "chat_presence"                 => "ChatPresenceUpdate",
        Presence                 => "presence"                      => "PresenceUpdate",
        PictureUpdate            => "picture_update"                => "PictureUpdate",
        UserAboutUpdate          => "user_about_update"             => "UserAboutUpdate",
        ContactUpdated           => "contact_updated"               => "ContactUpdated",
        ContactNumberChanged     => "contact_number_changed"        => "ContactNumberChanged",
        ContactSyncRequested     => "contact_sync_requested"        => "ContactSyncRequested",
        GroupUpdate              => "group_update"                  => "GroupUpdate",
        ContactUpdate            => "contact_update"                => "ContactUpdate",
        PushNameUpdate           => "push_name_update"              => "PushNameUpdate",
        SelfPushNameUpdated      => "self_push_name_updated"        => "SelfPushNameUpdated",
        PinUpdate                => "pin_update"                    => "PinUpdate",
        MuteUpdate               => "mute_update"                  => "MuteUpdate",
        ArchiveUpdate            => "archive_update"                => "ArchiveUpdate",
        StarUpdate               => "star_update"                   => "StarUpdate",
        MarkChatAsReadUpdate     => "mark_chat_as_read_update"      => "MarkChatAsReadUpdate",
        DeleteChatUpdate         => "delete_chat_update"            => "DeleteChatUpdate",
        DeleteMessageForMeUpdate => "delete_message_for_me_update"  => "DeleteMessageForMeUpdate",
        HistorySync              => "history_sync"                  => "Record<string, unknown>",
        OfflineSyncPreview       => "offline_sync_preview"          => "OfflineSyncPreview",
        OfflineSyncCompleted     => "offline_sync_completed"        => "OfflineSyncCompleted",
        DeviceListUpdate         => "device_list_update"            => "DeviceListUpdate",
        BusinessStatusUpdate     => "business_status_update"        => "BusinessStatusUpdate",
        TemporaryBan             => "temporary_ban"                 => "TemporaryBan",
        ConnectFailure           => "connect_failure"               => "ConnectFailure",
        StreamError              => "stream_error"                  => "StreamError",
        DisappearingModeChanged  => "disappearing_mode_changed"     => "DisappearingModeChanged",
        NewsletterLiveUpdate     => "newsletter_live_update"        => "NewsletterLiveUpdate",
        IncomingCall             => "incoming_call"                 => "IncomingCall",
    }
    special {
        // "js_name"                         => "TsDataType"
        "connected"                       => "Record<string, never>",
        "disconnected"                    => "Record<string, never>",
        "qr"                              => "{ code: string; timeout: number }",
        "pairing_code"                    => "{ code: string; timeout: number }",
        "pair_success"                    => "{ id: string; lid: string; business_name: string; platform: string }",
        "pair_error"                      => "{ id: string; lid: string; business_name: string; platform: string; error: string }",
        "logged_out"                      => "{ on_connect: boolean; reason: string }",
        "message"                         => "{ message: Record<string, unknown>; info: MessageInfo }",
        "notification"                    => "{ tag: string; attrs: Record<string, string>; content?: unknown }",
        "stream_replaced"                 => "Record<string, never>",
        "qr_scanned_without_multidevice"  => "Record<string, never>",
        "client_outdated"                 => "Record<string, never>",
        "raw_node"                        => "{ tag: string; attrs: Record<string, string>; content?: unknown }",
    }
}

// Declaration for the participant variant carried inside event-time
// `GroupNotificationAction.participants[]`. Mirrors the wire shape of
// `wacore::stanza::groups::GroupParticipantInfo` — fields stay as nested `Jid`
// objects (matching the rest of the event payload) instead of being flattened
// to strings, which is the whole point of keeping it separate from
// `GroupMetadataParticipant` (the cached-metadata variant returned by
// `getGroupMetadata`).
#[wasm_bindgen(typescript_custom_section)]
const _TS_GROUP_PARTICIPANT_INFO: &str = r#"
/**
 * Participant info as carried inside an event-time `GroupNotificationAction`.
 * Distinct from `GroupMetadataParticipant` (returned by `getGroupMetadata`,
 * which carries stringified `jid`/`phoneNumber` plus `isAdmin`).
 */
export interface GroupParticipantInfo {
  jid: Jid;
  phone_number?: Jid | null;
}
"#;

// Declarations for the incoming-call event types. These come from
// `wacore::types::call`, which is not tsify-derived in the bridge crate, so
// the wasm-bindgen output references them without declaring them. Without
// this block the `WhatsAppEvent` union has a dangling reference to
// `IncomingCall`. Shapes mirror the `Serialize` impls 1:1 — including
// `timestamp` being seconds-since-epoch (`#[serde(with = "ts_seconds")]`)
// rather than an ISO string.
#[wasm_bindgen(typescript_custom_section)]
const _TS_INCOMING_CALL: &str = r#"
/** One audio codec advertised inside a call `<offer>` child. */
export interface CallAudioCodec {
  enc: string;
  rate: number;
}

/**
 * Lifecycle action carried inside an inbound `<call>` stanza. The discriminant
 * (`type`) matches the stanza child name; `pre_accept` is the snake_case form
 * of `<pre-accept>`.
 */
export type CallAction =
  | {
      type: "offer";
      call_id: string;
      call_creator: Jid;
      caller_pn?: Jid | null;
      caller_country_code?: string | null;
      device_class?: string | null;
      joinable: boolean;
      is_video: boolean;
      audio: CallAudioCodec[];
    }
  | { type: "pre_accept"; call_id: string; call_creator: Jid }
  | { type: "accept"; call_id: string; call_creator: Jid }
  | { type: "reject"; call_id: string; call_creator: Jid }
  | {
      type: "terminate";
      call_id: string;
      call_creator: Jid;
      duration?: number | null;
      audio_duration?: number | null;
    };

/**
 * Inbound `<call>` stanza parsed into a typed event. `timestamp` is unix
 * seconds (not ISO) because the core serializes via
 * `chrono::serde::ts_seconds`.
 */
export interface IncomingCall {
  from: Jid;
  /** Stanza-level `id`; distinct from `CallAction.call_id`. */
  stanza_id: string;
  notify?: string | null;
  platform?: string | null;
  version?: string | null;
  timestamp: number;
  offline: boolean;
  action: CallAction;
}
"#;

#[wasm_bindgen(typescript_custom_section)]
const _TS_CLIENT_CONFIG: &str = r#"
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

/**
 * Initialize the WASM engine. Call once before creating clients.
 * @param logger Optional pino-compatible logger.
 * @param crypto Optional native crypto callbacks — when provided, AES/HMAC
 *               primitives delegate to the host (e.g. `node:crypto`). Falls
 *               back to the Rust-soft implementation if omitted.
 */
export function initWasmEngine(logger?: any, crypto?: JsCryptoCallbacks): void;

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
  cache_config?: CacheConfig | null,
): Promise<WasmWhatsAppClient>;

/** Cache entry configuration. */
export interface CacheEntryConfig {
  ttlSecs?: number;
  capacity?: number;
  store?: JsCacheStore;
}

/** Custom cache backend. */
export interface JsCacheStore {
  get(namespace: string, key: string): Promise<Uint8Array | null>;
  set(namespace: string, key: string, value: Uint8Array, ttlSecs?: number): Promise<void>;
  delete(namespace: string, key: string): Promise<void>;
  clear(namespace: string): Promise<void>;
}

/** Cache configuration — all fields optional. */
export interface CacheConfig {
  store?: JsCacheStore;
  group?: CacheEntryConfig;
  device?: CacheEntryConfig;
  deviceRegistry?: CacheEntryConfig;
  lidPn?: CacheEntryConfig;
  retriedGroupMessages?: CacheEntryConfig;
  recentMessages?: CacheEntryConfig;
  messageRetry?: CacheEntryConfig;
}

// Augment WasmWhatsAppClient with methods that need skip_typescript
// (Record returns can't be expressed by wasm-bindgen)
interface WasmWhatsAppClient {
  /** Fetch all groups the user is participating in. */
  groupFetchAllParticipating(): Promise<Record<string, GroupMetadataResult>>;
  /** Fetch user info for one or more JIDs. */
  fetchUserInfo(jids: string[]): Promise<Record<string, UserInfoResult>>;
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
    fn handle_event(&self, event: Arc<Event>) {
        match event_to_js(&event) {
            Ok(js_event) => {
                if js_event.is_undefined() {
                    return; // unhandled variant, already logged
                }
                if let Err(e) = self.event_tx.try_send(js_event) {
                    log::warn!("Event channel send failed: {e}");
                }
            }
            Err(e) => log::warn!("Event serialization failed: {e:?}"),
        }
    }
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
        Event::Notification(node) => {
            let data = node_ref_to_js(node.get())?;
            ("notification", data)
        }
        Event::RawNode(node) => {
            let data = node_ref_to_js(node.get())?;
            ("raw_node", data)
        }
        // All other variants are handled by serialize_event! in event_to_js
        other => {
            log::warn!(
                "unhandled event variant in event_to_js_special: {:?}",
                other
            );
            return Ok(JsValue::UNDEFINED);
        }
    };

    js_sys::Reflect::set(&obj, &"type".into(), &event_type.into())?;
    js_sys::Reflect::set(&obj, &"data".into(), &data)?;
    Ok(obj.into())
}

/// Host-agnostic equivalent of `event_to_js_special`.
fn event_to_json_special(event: &Event) -> Result<serde_json::Value, String> {
    let (event_type, data) = match event {
        Event::Connected(_) => ("connected", serde_json::json!({})),
        Event::Disconnected(_) => ("disconnected", serde_json::json!({})),
        Event::QrScannedWithoutMultidevice(_) => {
            ("qr_scanned_without_multidevice", serde_json::json!({}))
        }
        Event::ClientOutdated(_) => ("client_outdated", serde_json::json!({})),
        Event::StreamReplaced(_) => ("stream_replaced", serde_json::json!({})),
        Event::PairingQrCode { code, timeout } => (
            "qr",
            serde_json::json!({ "code": code, "timeout": timeout.as_secs() }),
        ),
        Event::PairingCode { code, timeout } => (
            "pairing_code",
            serde_json::json!({ "code": code, "timeout": timeout.as_secs() }),
        ),
        Event::PairSuccess(ps) => (
            "pair_success",
            serde_json::json!({
                "id": ps.id.to_string(),
                "lid": ps.lid.to_string(),
                "business_name": ps.business_name.as_str(),
                "platform": ps.platform.as_str(),
            }),
        ),
        Event::PairError(pe) => (
            "pair_error",
            serde_json::json!({
                "id": pe.id.to_string(),
                "lid": pe.lid.to_string(),
                "business_name": pe.business_name.as_str(),
                "platform": pe.platform.as_str(),
                "error": pe.error.as_str(),
            }),
        ),
        Event::LoggedOut(lo) => (
            "logged_out",
            serde_json::json!({
                "on_connect": lo.on_connect,
                "reason": format!("{:?}", lo.reason),
            }),
        ),
        Event::Message(msg, info) => {
            let msg_json = crate::camel_serializer::to_json_value_camel(msg.as_ref())?;
            let info_json = serde_json::to_value(info).map_err(|e| e.to_string())?;
            (
                "message",
                serde_json::json!({ "message": msg_json, "info": info_json }),
            )
        }
        Event::Notification(node) => {
            let val = serde_json::to_value(node.get()).map_err(|e| e.to_string())?;
            ("notification", val)
        }
        Event::RawNode(node) => {
            let val = serde_json::to_value(node.get()).map_err(|e| e.to_string())?;
            ("raw_node", val)
        }
        _other => {
            return Ok(serde_json::Value::Null);
        }
    };
    Ok(serde_json::json!({ "type": event_type, "data": data }))
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
pub fn init_wasm_engine(logger: JsValue, crypto: JsValue) {
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

    if let Err(e) = crate::js_crypto::try_install_from_js(&crypto) {
        log::warn!("skipping native crypto provider: {e:?}");
    }
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
) -> Result<WasmWhatsAppClient, JsError> {
    let runtime = Arc::new(WasmRuntime) as Arc<dyn wacore::runtime::Runtime>;
    let backend = match store {
        Some(ref store_val) if !store_val.is_null() && !store_val.is_undefined() => {
            let get_fn = js_sys::Reflect::get(store_val, &"get".into())
                .map_err(|_| JsError::new("store.get is required"))?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| JsError::new("store.get must be a function"))?;
            let set_fn = js_sys::Reflect::get(store_val, &"set".into())
                .map_err(|_| JsError::new("store.set is required"))?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| JsError::new("store.set must be a function"))?;
            let delete_fn = js_sys::Reflect::get(store_val, &"delete".into())
                .map_err(|_| JsError::new("store.delete is required"))?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| JsError::new("store.delete must be a function"))?;
            info!("Using JS-backed persistent storage");
            js_backend::new_js_backend(get_fn, set_fn, delete_fn)
        }
        _ => {
            info!("Using in-memory storage (no persistence)");
            js_backend::new_in_memory_backend()
        }
    };
    let transport_factory =
        Arc::new(JsTransportFactory::from_js(transport_config).map_err(js_val_err)?)
            as Arc<dyn wacore::net::TransportFactory>;
    let http_client = Arc::new(JsHttpClientAdapter::from_js(http_config).map_err(js_val_err)?)
        as Arc<dyn wacore::net::HttpClient>;

    let persistence_manager: Arc<whatsapp_rust::store::persistence_manager::PersistenceManager> =
        Arc::new(
            whatsapp_rust::store::persistence_manager::PersistenceManager::new(backend.clone())
                .await
                .map_err(|e| JsError::new(&format!("create persistence manager: {e}")))?,
        );

    let cache_config = build_cache_config(cache_config_js.as_ref()).map_err(js_val_err)?;

    let (client, sync_rx) = whatsapp_rust::Client::new_with_cache_config(
        runtime.clone(),
        persistence_manager.clone(),
        transport_factory,
        http_client,
        Some(DEFAULT_WA_WEB_VERSION),
        cache_config,
    )
    .await;

    // Start the periodic saver AFTER the Client exists so we can subscribe to
    // its shutdown signal. The returned `AbortHandle` is stored on the wrapper
    // and also aborted explicitly in `disconnect()` — belt-and-suspenders with
    // the self-terminating shutdown arm in the saver loop.
    let saver_handle = persistence_manager.clone().run_background_saver(
        runtime.clone(),
        std::time::Duration::from_secs(5),
        client.shutdown_signal(),
    );

    if let Some(callback) = on_event {
        let handler = Arc::new(JsEventHandler::new(callback)) as Arc<dyn EventHandler>;
        client.register_handler(handler);
    }

    Ok(WasmWhatsAppClient {
        client,
        runtime,
        sync_rx: Some(sync_rx),
        persistence_manager,
        saver_handle: Mutex::new(Some(saver_handle)),
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
    /// Handle to the bridge-owned background saver task. Aborted on
    /// `disconnect()` so the in-flight 5s `sleep` doesn't keep the Node.js
    /// event loop alive.
    saver_handle: Mutex<Option<wacore::runtime::AbortHandle>>,
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
    pub fn run(&mut self) -> Result<(), JsError> {
        if self.sync_rx.is_none() {
            return Err(JsError::new("run() has already been called"));
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
    pub async fn connect(&self) -> Result<(), JsError> {
        self.client.connect().await.map_err(js_err)
    }

    /// Disconnect the client and flush pending state to storage.
    pub async fn disconnect(&self) {
        self.client.disconnect().await;
        // Abort the bridge background saver BEFORE the final flush: aborting
        // drops its pending `sleep` future, which calls `clearTimeout` on the
        // 5s timer that would otherwise keep the Node.js event loop alive.
        // The explicit `flush()` below persists any dirty state the saver
        // would have written on its next tick.
        if let Some(handle) = self
            .saver_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            handle.abort();
        }
        if let Err(e) = self.persistence_manager.flush().await {
            log::warn!("Failed to flush state on disconnect: {e}");
        }
    }

    /// Logout from WhatsApp — deregisters this companion device and disconnects.
    ///
    /// Sends `remove-companion-device` IQ to the server (best-effort),
    /// then disconnects. Does NOT clear stored keys — the caller should
    /// delete the store to fully clear credentials.
    pub async fn logout(&self) -> Result<(), JsError> {
        self.client.logout().await.map_err(js_err)?;
        if let Some(handle) = self
            .saver_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            handle.abort();
        }
        if let Err(e) = self.persistence_manager.flush().await {
            log::warn!("Failed to flush state on logout: {e}");
        }
        Ok(())
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
    ) -> Result<String, JsError> {
        use whatsapp_rust::pair_code::PairCodeOptions;
        let options = PairCodeOptions {
            phone_number: phone_number.to_string(),
            custom_code,
            ..Default::default()
        };
        let code = self.client.pair_with_code(options).await.map_err(js_err)?;
        Ok(code)
    }

    // ── Sending messages ─────────────────────────────────────────────────

    /// Send an E2E encrypted message from protobuf bytes.
    /// Use `encodeProto('Message', obj)` on the JS side to produce the bytes.
    #[wasm_bindgen(js_name = sendMessageBytes)]
    pub async fn send_message_bytes(&self, jid: &str, bytes: &[u8]) -> Result<String, JsError> {
        let (to, msg) = parse_jid_and_msg_bytes(jid, bytes)?;
        let result = self.client.send_message(to, msg).await.map_err(js_err)?;
        Ok(result.message_id)
    }

    /// Low-level message relay from protobuf binary bytes.
    #[wasm_bindgen(js_name = relayMessageBytes)]
    pub async fn relay_message_bytes(
        &self,
        jid: &str,
        bytes: &[u8],
        message_id: Option<String>,
    ) -> Result<String, JsError> {
        let (to, msg) = parse_jid_and_msg_bytes(jid, bytes)?;
        let options = whatsapp_rust::SendOptions {
            message_id,
            ..Default::default()
        };
        let result = self
            .client
            .send_message_with_options(to, msg, options)
            .await
            .map_err(js_err)?;
        Ok(result.message_id)
    }

    // ── Message management ──────────────────────────────────────────────

    /// Edit a previously sent message from protobuf bytes.
    #[wasm_bindgen(js_name = editMessageBytes)]
    pub async fn edit_message_bytes(
        &self,
        jid: &str,
        message_id: &str,
        bytes: &[u8],
    ) -> Result<String, JsError> {
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
    ) -> Result<(), JsError> {
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
    pub async fn group_metadata(
        &self,
        jid: &str,
    ) -> Result<crate::result_types::GroupMetadataResult, JsError> {
        let group_jid = parse_jid(jid)?;

        let metadata = self
            .client
            .groups()
            .get_metadata(&group_jid)
            .await
            .map_err(js_err)?;

        Ok(group_metadata_to_result(&metadata))
    }

    /// Create a new group.
    ///
    /// Returns an object with `{ gid: string }`.
    #[wasm_bindgen(js_name = createGroup)]
    pub async fn group_create(
        &self,
        subject: &str,
        participants: Vec<String>,
    ) -> Result<crate::result_types::CreateGroupResult, JsError> {
        use whatsapp_rust::features::GroupParticipantOptions;

        let participant_options: Vec<GroupParticipantOptions> = participants
            .iter()
            .map(|p| parse_jid(p).map(GroupParticipantOptions::new))
            .collect::<Result<_, _>>()?;

        let options = whatsapp_rust::features::GroupCreateOptions::new(subject)
            .with_participants(participant_options);

        let result = self
            .client
            .groups()
            .create_group(options)
            .await
            .map_err(js_err)?;

        Ok(crate::result_types::CreateGroupResult {
            gid: result.gid.to_string(),
        })
    }

    /// Update a group's subject (name).
    #[wasm_bindgen(js_name = groupUpdateSubject)]
    pub async fn group_update_subject(&self, jid: &str, subject: &str) -> Result<(), JsError> {
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
    ) -> Result<(), JsError> {
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
    pub async fn group_leave(&self, jid: &str) -> Result<(), JsError> {
        let group_jid = parse_jid(jid)?;

        self.client.groups().leave(&group_jid).await.map_err(js_err)
    }

    /// Set or clear the bot's per-group "member label" — the small tag rendered
    /// under the bot's display name inside that group's UI. Empty `label`
    /// clears the label. The core sends this as a `ProtocolMessage` over the
    /// normal message path (not an IQ), matching WA Web's behavior.
    #[wasm_bindgen(js_name = updateMemberLabel)]
    pub async fn update_member_label(&self, group_jid: &str, label: &str) -> Result<(), JsError> {
        let parsed = parse_jid(group_jid)?;
        self.client
            .groups()
            .update_member_label(&parsed, label)
            .await
            .map_err(js_err)
    }

    /// Update group participants (add, remove, promote, demote).
    #[wasm_bindgen(js_name = groupParticipantsUpdate)]
    pub async fn group_participants_update(
        &self,
        jid: &str,
        participants: Vec<String>,
        action: crate::result_types::GroupParticipantAction,
    ) -> Result<Vec<crate::result_types::ParticipantChangeResult>, JsError> {
        use crate::result_types::GroupParticipantAction;
        let group_jid = parse_jid(jid)?;

        let participant_jids: Vec<Jid> = participants
            .iter()
            .map(|p| parse_jid(p))
            .collect::<Result<_, _>>()?;

        let to_results =
            |responses: Vec<whatsapp_rust::features::ParticipantChangeResponse>| -> Vec<crate::result_types::ParticipantChangeResult> {
                responses
                    .iter()
                    .map(|r| crate::result_types::ParticipantChangeResult {
                        jid: r.jid.to_string(),
                        status: r.status.clone(),
                        error: r.error.clone(),
                    })
                    .collect()
            };

        match action {
            GroupParticipantAction::Add => {
                let result = self
                    .client
                    .groups()
                    .add_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(js_err)?;
                Ok(to_results(result))
            }
            GroupParticipantAction::Remove => {
                let result = self
                    .client
                    .groups()
                    .remove_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(js_err)?;
                Ok(to_results(result))
            }
            GroupParticipantAction::Promote => {
                self.client
                    .groups()
                    .promote_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(js_err)?;
                Ok(Vec::new())
            }
            GroupParticipantAction::Demote => {
                self.client
                    .groups()
                    .demote_participants(&group_jid, &participant_jids)
                    .await
                    .map_err(js_err)?;
                Ok(Vec::new())
            }
        }
    }

    /// Fetch all groups the user is participating in.
    #[wasm_bindgen(js_name = groupFetchAllParticipating, skip_typescript)]
    pub async fn group_fetch_all_participating(&self) -> Result<JsValue, JsError> {
        let groups = self
            .client
            .groups()
            .get_participating()
            .await
            .map_err(js_err)?;

        let obj = js_sys::Object::new();
        for (key, metadata) in &groups {
            let result = group_metadata_to_result(metadata);
            let js_metadata = serde_wasm_bindgen::to_value(&result).map_err(js_err)?;
            js_sys::Reflect::set(&obj, &JsValue::from_str(key), &js_metadata)
                .map_err(js_val_err)?;
        }
        Ok(obj.into())
    }

    /// Get the invite link for a group.
    #[wasm_bindgen(js_name = groupInviteCode)]
    pub async fn group_invite_code(&self, jid: &str) -> Result<String, JsError> {
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
        setting: crate::result_types::GroupSetting,
        value: bool,
    ) -> Result<(), JsError> {
        use crate::result_types::GroupSetting;
        let group_jid = parse_jid(jid)?;

        match setting {
            GroupSetting::Locked => self
                .client
                .groups()
                .set_locked(&group_jid, value)
                .await
                .map_err(js_err)?,
            GroupSetting::Announce => self
                .client
                .groups()
                .set_announce(&group_jid, value)
                .await
                .map_err(js_err)?,
            GroupSetting::MembershipApproval => {
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
        }

        Ok(())
    }

    /// Set disappearing messages timer for a group (0 to disable).
    #[wasm_bindgen(js_name = groupToggleEphemeral)]
    pub async fn group_toggle_ephemeral(&self, jid: &str, expiration: u32) -> Result<(), JsError> {
        let group_jid = parse_jid(jid)?;
        self.client
            .groups()
            .set_ephemeral(&group_jid, expiration)
            .await
            .map_err(js_err)
    }

    /// Revoke a group's invite link (generates new one).
    #[wasm_bindgen(js_name = groupRevokeInvite)]
    pub async fn group_revoke_invite(&self, jid: &str) -> Result<String, JsError> {
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

    /// Check if one or more phone numbers / JIDs are registered on WhatsApp.
    ///
    /// Accepts either bare phone numbers (treated as PN JIDs) or full JIDs
    /// (`@s.whatsapp.net` for PN, `@lid` for LID). Mixed PN/LID inputs are
    /// transparently split into the two underlying usync queries by the core,
    /// so a single call is at most two IQs regardless of input size.
    ///
    /// Returns one `IsOnWhatsAppResult` per server hit — including the LID
    /// counterpart and business flag — eliminating the follow-up `fetchUserInfo`
    /// round trip the previous single-phone API forced callers into.
    #[wasm_bindgen(js_name = isOnWhatsApp)]
    pub async fn is_on_whatsapp(
        &self,
        phones: Vec<String>,
    ) -> Result<Vec<crate::result_types::IsOnWhatsAppResult>, JsError> {
        let jids: Vec<Jid> = phones
            .iter()
            .map(|p| {
                // Bare digits → PN JID; anything containing '@' → parse as full JID.
                if p.contains('@') {
                    parse_jid(p)
                } else {
                    Ok(Jid::pn(p))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        let results = self
            .client
            .contacts()
            .is_on_whatsapp(&jids)
            .await
            .map_err(js_err)?;

        // Use `Jid::push_to` instead of `to_string()` — bypasses the
        // `fmt::Display` / `dyn Write` dispatch path the core ships a specialized
        // fast writer for. Each output `String` is still owned (required to cross
        // the WASM ABI), but we skip the formatter machinery and size the buffer
        // up front to avoid mid-push reallocations.
        fn jid_to_owned(jid: &Jid) -> String {
            let mut buf = String::with_capacity(jid.user.len() + jid.server.as_str().len() + 8);
            jid.push_to(&mut buf);
            buf
        }

        Ok(results
            .iter()
            .map(|r| crate::result_types::IsOnWhatsAppResult {
                jid: jid_to_owned(&r.jid),
                is_registered: r.is_registered,
                lid: r.lid.as_ref().map(jid_to_owned),
                pn_jid: r.pn_jid.as_ref().map(jid_to_owned),
                is_business: r.is_business,
            })
            .collect())
    }

    /// Get the profile picture URL for a user or group.
    ///
    /// `picture_type` should be "preview" or "image".
    #[wasm_bindgen(js_name = profilePictureUrl)]
    pub async fn profile_picture_url(
        &self,
        jid: &str,
        picture_type: crate::result_types::PictureType,
    ) -> Result<Option<crate::result_types::ProfilePictureInfo>, JsError> {
        use crate::result_types::PictureType;
        let target = parse_jid(jid)?;
        let preview = match picture_type {
            PictureType::Preview => true,
            PictureType::Image => false,
        };

        let result = self
            .client
            .contacts()
            .get_profile_picture(&target, preview)
            .await
            .map_err(js_err)?;

        Ok(result.map(|pic| crate::result_types::ProfilePictureInfo {
            id: pic.id,
            url: pic.url,
            direct_path: pic.direct_path,
            hash: pic.hash,
        }))
    }

    /// Fetch user info for one or more JIDs.
    #[wasm_bindgen(js_name = fetchUserInfo, skip_typescript)]
    pub async fn fetch_user_info(&self, jids: Vec<String>) -> Result<JsValue, JsError> {
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
            let entry = crate::result_types::UserInfoResult {
                jid: info.jid.to_string(),
                lid: info.lid.as_ref().map(|l| l.to_string()),
                status: info.status.clone(),
                picture_id: info.picture_id.clone(),
                is_business: info.is_business,
            };
            let js_entry = serde_wasm_bindgen::to_value(&entry).map_err(js_err)?;
            js_sys::Reflect::set(&obj, &JsValue::from_str(&jid.to_string()), &js_entry)
                .map_err(js_val_err)?;
        }
        Ok(obj.into())
    }

    // ── Profile ──────────────────────────────────────────────────────────

    /// Set the user's push name (display name).
    #[wasm_bindgen(js_name = setPushName)]
    pub async fn set_push_name(&self, name: &str) -> Result<(), JsError> {
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
    ) -> Result<crate::result_types::ProfilePictureResult, JsError> {
        let result = self
            .client
            .profile()
            .set_profile_picture(img_data)
            .await
            .map_err(js_err)?;

        Ok(crate::result_types::ProfilePictureResult { id: result.id })
    }

    /// Remove the profile picture for the logged-in user.
    #[wasm_bindgen(js_name = removeProfilePicture)]
    pub async fn remove_profile_picture(
        &self,
    ) -> Result<crate::result_types::ProfilePictureResult, JsError> {
        let result = self
            .client
            .profile()
            .remove_profile_picture()
            .await
            .map_err(js_err)?;

        Ok(crate::result_types::ProfilePictureResult { id: result.id })
    }

    /// Set the profile picture for a group the user administers.
    ///
    /// Mirrors the core `SetProfilePictureSpec::set_group` path — same IQ as
    /// the self update, just routed at the JID level so admins can change a
    /// group's avatar from JS without an extra capability check.
    #[wasm_bindgen(js_name = setGroupProfilePicture)]
    pub async fn set_group_profile_picture(
        &self,
        group_jid: &str,
        img_data: Vec<u8>,
    ) -> Result<crate::result_types::ProfilePictureResult, JsError> {
        use wacore_binary::JidExt;
        let target = parse_jid(group_jid)?;
        if !target.is_group() {
            return Err(JsError::new(
                "setGroupProfilePicture: target jid must be a group jid",
            ));
        }
        let result = self
            .client
            .execute(wacore::iq::contacts::SetProfilePictureSpec::set_group(
                &target, img_data,
            ))
            .await
            .map_err(js_err)?;
        Ok(crate::result_types::ProfilePictureResult { id: result.id })
    }

    /// Remove a group's profile picture.
    #[wasm_bindgen(js_name = removeGroupProfilePicture)]
    pub async fn remove_group_profile_picture(
        &self,
        group_jid: &str,
    ) -> Result<crate::result_types::ProfilePictureResult, JsError> {
        use wacore_binary::JidExt;
        let target = parse_jid(group_jid)?;
        if !target.is_group() {
            return Err(JsError::new(
                "removeGroupProfilePicture: target jid must be a group jid",
            ));
        }
        let result = self
            .client
            .execute(wacore::iq::contacts::SetProfilePictureSpec::remove_group(
                &target,
            ))
            .await
            .map_err(js_err)?;
        Ok(crate::result_types::ProfilePictureResult { id: result.id })
    }

    /// Update the user's status text (about).
    #[wasm_bindgen(js_name = updateProfileStatus)]
    pub async fn update_profile_status(&self, status: &str) -> Result<(), JsError> {
        self.client
            .profile()
            .set_status_text(status)
            .await
            .map_err(js_err)
    }

    // ── Blocking ──────────────────────────────────────────────────────────

    /// Block or unblock a contact.
    #[wasm_bindgen(js_name = updateBlockStatus)]
    pub async fn update_block_status(
        &self,
        jid: &str,
        action: crate::result_types::BlockAction,
    ) -> Result<(), JsError> {
        use crate::result_types::BlockAction;
        let target = parse_jid(jid)?;

        match action {
            BlockAction::Block => self
                .client
                .blocking()
                .block(&target)
                .await
                .map_err(js_err)?,
            BlockAction::Unblock => self
                .client
                .blocking()
                .unblock(&target)
                .await
                .map_err(js_err)?,
        }
        Ok(())
    }

    /// Fetch the full blocklist.
    #[wasm_bindgen(js_name = fetchBlocklist)]
    pub async fn fetch_blocklist(
        &self,
    ) -> Result<Vec<crate::result_types::BlocklistEntryResult>, JsError> {
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
    pub async fn pin_chat(&self, jid: &str, pin: bool) -> Result<(), JsError> {
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
    pub async fn mute_chat(&self, jid: &str, mute_until: Option<f64>) -> Result<(), JsError> {
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
    pub async fn archive_chat(&self, jid: &str, archive: bool) -> Result<(), JsError> {
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
    ) -> Result<(), JsError> {
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
    pub async fn mark_chat_as_read(&self, jid: &str, read: bool) -> Result<(), JsError> {
        let chat_jid = parse_jid(jid)?;
        self.client
            .chat_actions()
            .mark_chat_as_read(&chat_jid, read, None)
            .await
            .map_err(js_err)
    }

    /// Delete a chat via app state mutation.
    #[wasm_bindgen(js_name = deleteChat)]
    pub async fn delete_chat(&self, jid: &str) -> Result<(), JsError> {
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
    ) -> Result<(), JsError> {
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
    ) -> Result<crate::result_types::CreatePollResult, JsError> {
        let to = parse_jid(jid)?;
        let (result, message_secret) = self
            .client
            .polls()
            .create(&to, name, &options, selectable_count)
            .await
            .map_err(js_err)?;
        Ok(crate::result_types::CreatePollResult {
            message_id: result.message_id,
            message_secret: message_secret.to_vec(),
        })
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
    ) -> Result<String, JsError> {
        let chat = parse_jid(chat_jid)?;
        let creator = parse_jid(poll_creator_jid)?;
        let result = self
            .client
            .polls()
            .vote(&chat, poll_msg_id, &creator, message_secret, &option_names)
            .await
            .map_err(js_err)?;
        Ok(result.message_id)
    }

    /// Send a status/story message to specified recipients.
    /// Use `encodeProto('Message', obj)` on the JS side to produce the bytes.
    #[wasm_bindgen(js_name = sendStatusMessageBytes)]
    pub async fn send_status_message_bytes(
        &self,
        bytes: &[u8],
        recipients: Vec<String>,
    ) -> Result<String, JsError> {
        use prost::Message;
        let msg = waproto::whatsapp::Message::decode(bytes)
            .map_err(|e| JsError::new(&format!("invalid message bytes: {e}")))?;
        let jids: Vec<Jid> = recipients
            .iter()
            .map(|s| parse_jid(s))
            .collect::<Result<_, _>>()?;
        let result = self
            .client
            .status()
            .send_raw(msg, &jids, Default::default())
            .await
            .map_err(js_err)?;
        Ok(result.message_id)
    }

    // ── Read receipts ─────────────────────────────────────────────────

    /// Mark messages as read by sending read receipts.
    #[wasm_bindgen(js_name = readMessages)]
    pub async fn read_messages(
        &self,
        keys: Vec<crate::result_types::ReadMessageKey>,
    ) -> Result<(), JsError> {
        use std::collections::HashMap;
        let mut grouped: HashMap<(String, Option<String>), Vec<String>> = HashMap::new();

        for key in keys {
            grouped
                .entry((key.remote_jid, key.participant))
                .or_default()
                .push(key.id);
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
    pub async fn group_accept_invite(&self, code: &str) -> Result<String, JsError> {
        let jid = self
            .client
            .groups()
            .join_with_invite_code(code)
            .await
            .map_err(js_err)?;
        Ok(jid.group_jid().to_string())
    }

    /// Join a group via a GroupInviteMessage (V4 invite).
    #[wasm_bindgen(js_name = groupAcceptInviteV4)]
    pub async fn group_accept_invite_v4(
        &self,
        group_jid: &str,
        code: &str,
        expiration: f64,
        admin_jid: &str,
    ) -> Result<String, JsError> {
        let group = parse_jid(group_jid)?;
        let admin = parse_jid(admin_jid)?;
        let result = self
            .client
            .groups()
            .join_with_invite_v4(&group, code, expiration as i64, &admin)
            .await
            .map_err(js_err)?;
        Ok(result.group_jid().to_string())
    }

    /// Get group info from an invite code (without joining).
    /// Returns the same shape as groupMetadata.
    #[wasm_bindgen(js_name = groupGetInviteInfo)]
    pub async fn group_get_invite_info(
        &self,
        code: &str,
    ) -> Result<crate::result_types::GroupMetadataResult, JsError> {
        let metadata = self
            .client
            .groups()
            .get_invite_info(code)
            .await
            .map_err(js_err)?;
        Ok(group_metadata_to_result(&metadata))
    }

    /// Get list of pending join requests for a group.
    #[wasm_bindgen(js_name = groupRequestParticipantsList)]
    pub async fn group_request_participants_list(
        &self,
        jid: &str,
    ) -> Result<Vec<crate::result_types::MembershipRequestResult>, JsError> {
        let group_jid = parse_jid(jid)?;
        let list = self
            .client
            .groups()
            .get_membership_requests(&group_jid)
            .await
            .map_err(js_err)?;
        Ok(list
            .iter()
            .map(|r| crate::result_types::MembershipRequestResult {
                jid: r.jid.to_string(),
                request_time: r.request_time.map(|t| t as f64),
            })
            .collect())
    }

    /// Approve or reject pending join requests.
    #[wasm_bindgen(js_name = groupRequestParticipantsUpdate)]
    pub async fn group_request_participants_update(
        &self,
        jid: &str,
        participants: Vec<String>,
        action: crate::result_types::GroupRequestAction,
    ) -> Result<(), JsError> {
        use crate::result_types::GroupRequestAction;
        let group_jid = parse_jid(jid)?;
        let participant_jids: Vec<Jid> = participants
            .iter()
            .map(|s| parse_jid(s))
            .collect::<Result<Vec<_>, _>>()?;

        match action {
            GroupRequestAction::Approve => {
                self.client
                    .groups()
                    .approve_membership_requests(&group_jid, &participant_jids)
                    .await
                    .map_err(js_err)?;
            }
            GroupRequestAction::Reject => {
                self.client
                    .groups()
                    .reject_membership_requests(&group_jid, &participant_jids)
                    .await
                    .map_err(js_err)?;
            }
        }
        Ok(())
    }

    // ── Privacy settings ──────────────────────────────────────────────

    /// Fetch all privacy settings.
    #[wasm_bindgen(js_name = fetchPrivacySettings)]
    pub async fn fetch_privacy_settings(&self) -> Result<JsValue, JsError> {
        let response = self.client.fetch_privacy_settings().await.map_err(js_err)?;
        let map: std::collections::HashMap<&str, &str> = response
            .settings
            .iter()
            .map(|s| (s.category.as_str(), s.value.as_str()))
            .collect();
        serde_wasm_bindgen::to_value(&map).map_err(js_err)
    }

    /// Update a single privacy setting.
    #[wasm_bindgen(js_name = updatePrivacySetting)]
    pub async fn update_privacy_setting(&self, category: &str, value: &str) -> Result<(), JsError> {
        self.client
            .set_privacy_setting(category.into(), value.into())
            .await
            .map_err(js_err)?;
        Ok(())
    }

    /// Set default disappearing messages duration (seconds). 0 to disable.
    #[wasm_bindgen(js_name = updateDefaultDisappearingMode)]
    pub async fn update_default_disappearing_mode(&self, duration: u32) -> Result<(), JsError> {
        self.client
            .set_default_disappearing_mode(duration)
            .await
            .map_err(js_err)
    }

    // ── Calls ────────────────────────────────────────────────────────────

    /// Reject an incoming call.
    #[wasm_bindgen(js_name = rejectCall)]
    pub async fn reject_call(&self, call_id: &str, call_from: &str) -> Result<(), JsError> {
        let from_jid = parse_jid(call_from)?;
        self.client
            .reject_call(call_id, &from_jid)
            .await
            .map_err(js_err)
    }

    // ── User status ──────────────────────────────────────────────────────

    /// Fetch user status/about text for one or more JIDs.
    #[wasm_bindgen(js_name = fetchStatus)]
    pub async fn fetch_status(
        &self,
        jids: Vec<String>,
    ) -> Result<Vec<crate::result_types::FetchStatusResult>, JsError> {
        let parsed_jids: Vec<Jid> = jids
            .iter()
            .map(|s| parse_jid(s))
            .collect::<Result<_, _>>()?;
        let infos = self
            .client
            .contacts()
            .get_user_info(&parsed_jids)
            .await
            .map_err(js_err)?;
        Ok(infos
            .values()
            .map(|info| crate::result_types::FetchStatusResult {
                jid: info.jid.to_string(),
                status: info.status.clone(),
            })
            .collect())
    }

    // ── Business profile ───────────────────────────────────────────────

    /// Get business profile information for a JID.
    #[wasm_bindgen(js_name = getBusinessProfile)]
    pub async fn get_business_profile(
        &self,
        jid: &str,
    ) -> Result<Option<crate::result_types::BusinessProfileResult>, JsError> {
        let target_jid = parse_jid(jid)?;
        let profile = self
            .client
            .execute(wacore::iq::business::BusinessProfileSpec::new(&target_jid))
            .await
            .map_err(js_err)?;
        Ok(profile.map(|p| business_profile_to_result(&p)))
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
    ) -> Result<String, JsError> {
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
        Ok(msg_id)
    }

    // ── Group member add mode ────────────────────────────────────────────

    /// Set who can add members to a group.
    #[wasm_bindgen(js_name = groupMemberAddMode)]
    pub async fn group_member_add_mode(
        &self,
        jid: &str,
        mode: crate::result_types::MemberAddMode,
    ) -> Result<(), JsError> {
        use crate::result_types::MemberAddMode;
        let group_jid = parse_jid(jid)?;
        let add_mode = match mode {
            MemberAddMode::AdminAdd => whatsapp_rust::features::MemberAddMode::AdminAdd,
            MemberAddMode::AllMemberAdd => whatsapp_rust::features::MemberAddMode::AllMemberAdd,
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
    pub async fn send_presence(
        &self,
        status: crate::result_types::PresenceStatus,
    ) -> Result<(), JsError> {
        use crate::result_types::PresenceStatus;
        let presence_status = match status {
            PresenceStatus::Available => whatsapp_rust::features::PresenceStatus::Available,
            PresenceStatus::Unavailable => whatsapp_rust::features::PresenceStatus::Unavailable,
        };

        self.client
            .presence()
            .set(presence_status)
            .await
            .map_err(js_err)
    }

    /// Subscribe to a contact's presence updates.
    #[wasm_bindgen(js_name = presenceSubscribe)]
    pub async fn presence_subscribe(&self, jid: &str) -> Result<(), JsError> {
        let target = parse_jid(jid)?;

        self.client
            .presence()
            .subscribe(&target)
            .await
            .map_err(js_err)
    }

    // ── Newsletter ────────────────────────────────────────────────────────

    /// Create a new newsletter (channel).
    #[wasm_bindgen(js_name = newsletterCreate)]
    pub async fn newsletter_create(
        &self,
        name: &str,
        description: Option<String>,
    ) -> Result<crate::result_types::NewsletterMetadataResult, JsError> {
        let result = self
            .client
            .newsletter()
            .create(name, description.as_deref())
            .await
            .map_err(js_err)?;

        Ok(newsletter_metadata_to_result(&result))
    }

    /// Fetch metadata for a newsletter by JID.
    #[wasm_bindgen(js_name = newsletterMetadata)]
    pub async fn newsletter_metadata(
        &self,
        jid: &str,
    ) -> Result<crate::result_types::NewsletterMetadataResult, JsError> {
        let target = parse_jid(jid)?;

        let result = self
            .client
            .newsletter()
            .get_metadata(&target)
            .await
            .map_err(js_err)?;

        Ok(newsletter_metadata_to_result(&result))
    }

    /// Subscribe (join) a newsletter.
    #[wasm_bindgen(js_name = newsletterSubscribe)]
    pub async fn newsletter_subscribe(
        &self,
        jid: &str,
    ) -> Result<crate::result_types::NewsletterMetadataResult, JsError> {
        let target = parse_jid(jid)?;

        let result = self
            .client
            .newsletter()
            .join(&target)
            .await
            .map_err(js_err)?;

        Ok(newsletter_metadata_to_result(&result))
    }

    /// Unsubscribe (leave) a newsletter.
    #[wasm_bindgen(js_name = newsletterUnsubscribe)]
    pub async fn newsletter_unsubscribe(&self, jid: &str) -> Result<(), JsError> {
        let target = parse_jid(jid)?;

        self.client
            .newsletter()
            .leave(&target)
            .await
            .map_err(js_err)
    }

    /// Send a reaction to a newsletter message.
    ///
    /// `server_id` is the server-assigned message ID (passed as string to avoid
    /// JS number precision issues). `reaction` is the emoji code, or null/empty
    /// to remove a reaction.
    #[wasm_bindgen(js_name = newsletterReactMessage)]
    pub async fn newsletter_react_message(
        &self,
        jid: &str,
        server_id: &str,
        reaction: Option<String>,
    ) -> Result<(), JsError> {
        let target = parse_jid(jid)?;
        let sid: u64 = server_id
            .parse()
            .map_err(|e| JsError::new(&format!("invalid server_id: {e}")))?;
        self.client
            .newsletter()
            .send_reaction(&target, sid, reaction.as_deref().unwrap_or(""))
            .await
            .map_err(js_err)
    }

    // ── Media reupload ────────────────────────────────────────────────────

    /// Request the server to re-upload expired media.
    ///
    /// Returns the new `directPath` on success.
    /// Throws on failure (not found, decryption error, timeout, etc.).
    #[wasm_bindgen(js_name = requestMediaReupload)]
    pub async fn request_media_reupload(
        &self,
        msg_id: &str,
        chat_jid: &str,
        media_key: &[u8],
        is_from_me: bool,
        participant: Option<String>,
    ) -> Result<String, JsError> {
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
            whatsapp_rust::MediaRetryResult::Success { direct_path } => Ok(direct_path),
            whatsapp_rust::MediaRetryResult::NotFound => {
                Err(JsError::new("Media not found on server"))
            }
            whatsapp_rust::MediaRetryResult::DecryptionError => {
                Err(JsError::new("Media decryption error"))
            }
            whatsapp_rust::MediaRetryResult::GeneralError => {
                Err(JsError::new("Media reupload failed"))
            }
        }
    }

    // ── Chat state ───────────────────────────────────────────────────────

    /// Send a chat state update (typing indicator).
    #[wasm_bindgen(js_name = sendChatState)]
    pub async fn send_chat_state(
        &self,
        jid: &str,
        state: crate::result_types::ChatState,
    ) -> Result<(), JsError> {
        use crate::result_types::ChatState;
        let to = parse_jid(jid)?;

        let chat_state = match state {
            ChatState::Composing => whatsapp_rust::features::ChatStateType::Composing,
            ChatState::Recording => whatsapp_rust::features::ChatStateType::Recording,
            ChatState::Paused => whatsapp_rust::features::ChatStateType::Paused,
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
    ) -> Result<crate::result_types::MediaConnResult, JsError> {
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
        media_type: crate::result_types::MediaType,
    ) -> Result<js_sys::Uint8Array, JsError> {
        let mt: wacore::download::MediaType = media_type.into();
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
        media_type: crate::result_types::MediaType,
    ) -> Result<web_sys::ReadableStream, JsError> {
        let mt: wacore::download::MediaType = media_type.into();
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
    /// signing, multi-host CDN upload, auth refresh, and resumable upload (>=5MB).
    #[wasm_bindgen(js_name = uploadMedia)]
    pub async fn upload_media(
        &self,
        data: &[u8],
        media_type: crate::result_types::MediaType,
    ) -> Result<crate::result_types::UploadMediaResult, JsError> {
        let mt: wacore::download::MediaType = media_type.into();
        let resp = self
            .client
            .upload(data.to_vec(), mt, Default::default())
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

    /// True streaming encrypt via `MediaEncryptor`: processes plaintext chunk-by-chunk
    /// from JS ReadableStream, encrypts with AES-256-CBC, writes ciphertext to JS WritableStream.
    ///
    /// Peak memory: ~130KB (copy buffer + flush buffer + crypto state).
    #[wasm_bindgen(js_name = encryptMediaStream)]
    pub async fn encrypt_media_stream(
        &self,
        input: web_sys::ReadableStream,
        output: web_sys::WritableStream,
        media_type: crate::result_types::MediaType,
    ) -> Result<crate::result_types::EncryptMediaResult, JsError> {
        use futures::SinkExt;
        use futures::StreamExt;
        use wacore::upload::MediaEncryptor;

        let mt: wacore::download::MediaType = media_type.into();

        let rs = wasm_streams::ReadableStream::from_raw(input);
        let mut reader = rs.into_stream();
        let ws = wasm_streams::WritableStream::from_raw(output);
        let mut writer = ws.into_sink();

        const FLUSH_THRESHOLD: usize = 65536;

        let mut enc = MediaEncryptor::new(mt).map_err(js_err)?;
        let mut out_buf = Vec::with_capacity(FLUSH_THRESHOLD + 16);
        let mut copy_buf = vec![0u8; FLUSH_THRESHOLD];

        while let Some(chunk_result) = reader.next().await {
            let chunk = chunk_result.map_err(|e| JsError::new(&format!("read error: {e:?}")))?;
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
                    .map_err(|e| JsError::new(&format!("write error: {e:?}")))?;
                out_buf.clear();
            }
        }

        let info = enc.finalize(&mut out_buf).map_err(js_err)?;

        if !out_buf.is_empty() {
            let js_chunk = js_sys::Uint8Array::from(out_buf.as_slice());
            writer
                .send(js_chunk.into())
                .await
                .map_err(|e| JsError::new(&format!("write error: {e:?}")))?;
        }
        writer
            .close()
            .await
            .map_err(|e| JsError::new(&format!("close error: {e:?}")))?;

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
    /// Handles CDN failover, auth refresh, and resumable upload (>=5MB).
    #[wasm_bindgen(js_name = uploadEncryptedMediaStream)]
    pub async fn upload_encrypted_media_stream(
        &self,
        get_body: &js_sys::Function,
        media_key: &[u8],
        file_sha256: &[u8],
        file_enc_sha256: &[u8],
        file_length: f64,
        media_type: crate::result_types::MediaType,
    ) -> Result<crate::result_types::UploadMediaResult, JsError> {
        let mt: wacore::download::MediaType = media_type.into();
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
                            media_key: media_key.try_into().map_err(js_err)?,
                            file_sha256: file_sha256.try_into().map_err(js_err)?,
                            file_enc_sha256: file_enc_sha256.try_into().map_err(js_err)?,
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
                    .map_err(|e| JsError::new(&format!("getBody() failed: {e:?}")))?;

                // Try streaming upload via JS HTTP client
                let result = stream_upload_via_js(&self.client, &upload_url, body_stream).await;

                match result {
                    Ok(resp) if resp.status_code < 400 => {
                        let parsed: serde_json::Value =
                            serde_json::from_slice(&resp.body).map_err(js_err)?;
                        let url = parsed
                            .get("url")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| JsError::new("missing url in response"))?;
                        let dp = parsed
                            .get("direct_path")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| JsError::new("missing direct_path in response"))?;
                        return Ok(crate::result_types::UploadMediaResult {
                            url: url.to_string(),
                            direct_path: dp.to_string(),
                            media_key: media_key.try_into().map_err(js_err)?,
                            file_sha256: file_sha256.try_into().map_err(js_err)?,
                            file_enc_sha256: file_enc_sha256.try_into().map_err(js_err)?,
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

        Err(JsError::new("Upload failed on all hosts"))
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

    /// Get the ADV signed device identity (account), if available.
    /// Used by upstream Baileys consumers that access `authState.creds.account`.
    #[wasm_bindgen(js_name = getAccount)]
    pub async fn get_account(&self) -> Result<JsValue, JsError> {
        let snapshot = self.persistence_manager.get_device_snapshot().await;
        match &snapshot.account {
            Some(account) => crate::camel_serializer::to_js_value_camel(account)
                .map_err(|e| JsError::new(&format!("account serialization: {e:?}"))),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// Returns a snapshot of internal memory diagnostics (cache sizes, session counts, etc.).
    #[wasm_bindgen(js_name = getMemoryDiagnostics)]
    pub async fn get_memory_diagnostics(&self) -> crate::result_types::MemoryDiagnosticsResult {
        let d = self.client.memory_diagnostics().await;
        crate::result_types::MemoryDiagnosticsResult {
            group_cache: d.group_cache as f64,
            device_registry_cache: d.device_registry_cache as f64,
            sender_key_device_cache: d.sender_key_device_cache as f64,
            lid_pn_lid_entries: d.lid_pn_lid_entries as f64,
            lid_pn_pn_entries: d.lid_pn_pn_entries as f64,
            recent_messages: d.recent_messages as f64,
            message_retry_counts: d.message_retry_counts as f64,
            pdo_pending_requests: d.pdo_pending_requests as f64,
            session_locks: d.session_locks as f64,
            chat_lanes: d.chat_lanes as f64,
            response_waiters: d.response_waiters as f64,
            node_waiters: d.node_waiters as f64,
            pending_retries: d.pending_retries as f64,
            presence_subscriptions: d.presence_subscriptions as f64,
            app_state_key_requests: d.app_state_key_requests as f64,
            app_state_syncing: d.app_state_syncing as f64,
            signal_cache_sessions: d.signal_cache_sessions as f64,
            signal_cache_identities: d.signal_cache_identities as f64,
            signal_cache_sender_keys: d.signal_cache_sender_keys as f64,
            chatstate_handlers: d.chatstate_handlers as f64,
            custom_enc_handlers: d.custom_enc_handlers as f64,
        }
    }

    // ── Signal / low-level protocol ──────────────────────────────────────

    /// Enable or disable raw node forwarding. When enabled, a `raw_node` event
    /// is emitted for every decoded stanza before internal dispatch.
    #[wasm_bindgen(js_name = setRawNodeForwarding)]
    pub fn set_raw_node_forwarding(&self, enabled: bool) {
        self.client.set_raw_node_forwarding(enabled);
    }

    /// Send a raw binary node stanza to WhatsApp servers.
    /// Accepts a JS object matching `{ tag: string, attrs: Record<string, string>, content?: ... }`.
    #[wasm_bindgen(js_name = sendNode)]
    pub async fn send_node(&self, node_js: JsValue) -> Result<(), JsError> {
        let node = js_to_node(&node_js)?;
        self.client.send_node(node).await.map_err(js_err)
    }

    /// Ensure E2E Signal sessions exist for the given JIDs.
    /// Returns true after sessions are established.
    #[wasm_bindgen(js_name = assertSessions)]
    pub async fn assert_sessions(&self, jids: Vec<String>, _force: bool) -> Result<bool, JsError> {
        let parsed: Vec<wacore_binary::jid::Jid> = jids
            .iter()
            .map(|j| parse_jid(j))
            .collect::<Result<_, _>>()?;
        self.client
            .signal()
            .assert_sessions(&parsed)
            .await
            .map_err(js_err)?;
        Ok(true)
    }

    /// Get the list of known devices for the given user JIDs via usync query.
    /// Returns an array of JID strings (one per device).
    #[wasm_bindgen(js_name = getUSyncDevices)]
    pub async fn get_usync_devices(
        &self,
        jids: Vec<String>,
        _use_cache: bool,
        _ignore_zero_devices: bool,
    ) -> Result<JsValue, JsError> {
        let parsed: Vec<wacore_binary::jid::Jid> = jids
            .iter()
            .map(|j| parse_jid(j))
            .collect::<Result<_, _>>()?;
        let devices = self
            .client
            .signal()
            .get_user_devices(&parsed)
            .await
            .map_err(js_err)?;
        // Return as JidWithDevice[] = { user: string, device?: number, jid: string }
        let arr = js_sys::Array::new_with_length(devices.len() as u32);
        for (i, jid) in devices.iter().enumerate() {
            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&obj, &"user".into(), &jid.user.as_str().into())
                .map_err(|e| JsError::new(&format!("{e:?}")))?;
            if jid.device != 0 {
                js_sys::Reflect::set(&obj, &"device".into(), &(jid.device as f64).into())
                    .map_err(|e| JsError::new(&format!("{e:?}")))?;
            }
            js_sys::Reflect::set(&obj, &"jid".into(), &jid.to_string().into())
                .map_err(|e| JsError::new(&format!("{e:?}")))?;
            arr.set(i as u32, obj.into());
        }
        Ok(arr.into())
    }

    // ── Signal protocol ──────────────────────────────────────────────────

    /// Encrypt plaintext for a single recipient.
    /// Returns `{ type: "msg"|"pkmsg", ciphertext: Uint8Array }`.
    #[wasm_bindgen(js_name = signalEncryptMessage)]
    pub async fn signal_encrypt_message(&self, jid: &str, data: &[u8]) -> Result<JsValue, JsError> {
        let parsed = parse_jid(jid)?;
        let (msg_type, ciphertext) = self
            .client
            .signal()
            .encrypt_message(&parsed, data)
            .await
            .map_err(js_err)?;
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"type".into(), &msg_type.as_wire_str().into())
            .map_err(|e| JsError::new(&format!("{e:?}")))?;
        js_sys::Reflect::set(
            &obj,
            &"ciphertext".into(),
            &js_sys::Uint8Array::from(ciphertext.as_slice()).into(),
        )
        .map_err(|e| JsError::new(&format!("{e:?}")))?;
        Ok(obj.into())
    }

    /// Decrypt a Signal protocol message. `msg_type` is "msg", "pkmsg", or "skmsg".
    #[wasm_bindgen(js_name = signalDecryptMessage)]
    pub async fn signal_decrypt_message(
        &self,
        jid: &str,
        msg_type: &str,
        ciphertext: &[u8],
    ) -> Result<js_sys::Uint8Array, JsError> {
        let parsed = parse_jid(jid)?;
        let enc_type = wacore::message_processing::EncType::from_wire(msg_type)
            .ok_or_else(|| JsError::new(&format!("invalid msg_type: {msg_type}")))?;
        let plaintext = self
            .client
            .signal()
            .decrypt_message(&parsed, enc_type, ciphertext)
            .await
            .map_err(js_err)?;
        Ok(js_sys::Uint8Array::from(plaintext.as_slice()))
    }

    /// Encrypt plaintext for a group (sender key).
    /// Returns `{ senderKeyDistributionMessage: Uint8Array, ciphertext: Uint8Array }`.
    #[wasm_bindgen(js_name = signalEncryptGroupMessage)]
    pub async fn signal_encrypt_group_message(
        &self,
        group_jid: &str,
        data: &[u8],
        _me_id: &str,
    ) -> Result<JsValue, JsError> {
        let parsed = parse_jid(group_jid)?;
        let (skdm, ciphertext) = self
            .client
            .signal()
            .encrypt_group_message(&parsed, data)
            .await
            .map_err(js_err)?;
        let obj = js_sys::Object::new();
        let skdm_js = match &skdm {
            Some(bytes) => js_sys::Uint8Array::from(bytes.as_slice()).into(),
            None => JsValue::UNDEFINED,
        };
        js_sys::Reflect::set(&obj, &"senderKeyDistributionMessage".into(), &skdm_js)
            .map_err(|e| JsError::new(&format!("{e:?}")))?;
        js_sys::Reflect::set(
            &obj,
            &"ciphertext".into(),
            &js_sys::Uint8Array::from(ciphertext.as_slice()).into(),
        )
        .map_err(|e| JsError::new(&format!("{e:?}")))?;
        Ok(obj.into())
    }

    /// Decrypt a group (sender-key) message.
    #[wasm_bindgen(js_name = signalDecryptGroupMessage)]
    pub async fn signal_decrypt_group_message(
        &self,
        group_jid: &str,
        author_jid: &str,
        msg: &[u8],
    ) -> Result<js_sys::Uint8Array, JsError> {
        let group = parse_jid(group_jid)?;
        let sender = parse_jid(author_jid)?;
        let plaintext = self
            .client
            .signal()
            .decrypt_group_message(&group, &sender, msg)
            .await
            .map_err(js_err)?;
        Ok(js_sys::Uint8Array::from(plaintext.as_slice()))
    }

    /// Check whether a Signal session exists for the given JID.
    #[wasm_bindgen(js_name = signalValidateSession)]
    pub async fn signal_validate_session(&self, jid: &str) -> Result<bool, JsError> {
        let parsed = parse_jid(jid)?;
        self.client
            .signal()
            .validate_session(&parsed)
            .await
            .map_err(js_err)
    }

    /// Delete Signal sessions for the given JIDs.
    #[wasm_bindgen(js_name = signalDeleteSessions)]
    pub async fn signal_delete_sessions(&self, jids: Vec<String>) -> Result<(), JsError> {
        let parsed: Vec<wacore_binary::jid::Jid> = jids
            .iter()
            .map(|j| parse_jid(j))
            .collect::<Result<_, _>>()?;
        self.client
            .signal()
            .delete_sessions(&parsed)
            .await
            .map_err(js_err)
    }

    /// Look up the LID JID corresponding to a given phone number JID.
    ///
    /// Accepts a bare phone number (treated as PN), a `<phone>@s.whatsapp.net`
    /// JID, or any LID/PN JID. Returns the full LID JID string (e.g.
    /// `100000012345678@lid`) or `null` when no mapping is known. Backed by
    /// the core's cache-aside `get_lid_pn_entry`: hits the in-memory cache
    /// first, then falls through to `backend.get_pn_mapping(user)` so a JS
    /// `JsStoreCallbacks` backend without a list primitive still resolves
    /// every persisted mapping without an extra usync round trip.
    #[wasm_bindgen(js_name = lidForPn)]
    pub async fn lid_for_pn(&self, jid: &str) -> Result<Option<String>, JsError> {
        let parsed = if jid.contains('@') {
            parse_jid(jid)?
        } else {
            Jid::pn(jid)
        };
        Ok(self
            .client
            .get_lid_pn_entry(&parsed)
            .await
            .map_err(js_err)?
            .map(|e| format!("{}@lid", e.lid)))
    }

    /// Look up the phone number JID corresponding to a given LID JID.
    ///
    /// Accepts a bare LID user-part, a `<user>@lid` JID, or any LID/PN JID.
    /// Returns the full PN JID string (e.g. `559980000001@s.whatsapp.net`) or
    /// `null` when no mapping is known. Same cache-aside semantics as
    /// `lidForPn` — see that doc.
    #[wasm_bindgen(js_name = pnForLid)]
    pub async fn pn_for_lid(&self, jid: &str) -> Result<Option<String>, JsError> {
        let parsed = if jid.contains('@') {
            parse_jid(jid)?
        } else {
            Jid::lid(jid)
        };
        Ok(self
            .client
            .get_lid_pn_entry(&parsed)
            .await
            .map_err(js_err)?
            .map(|e| format!("{}@s.whatsapp.net", e.phone_number)))
    }

    /// Convert a JID string to its Signal protocol address representation.
    #[wasm_bindgen(js_name = jidToSignalProtocolAddress)]
    pub fn jid_to_signal_protocol_address(&self, jid: &str) -> Result<String, JsError> {
        use wacore::types::jid::JidExt;
        let parsed = parse_jid(jid)?;
        Ok(parsed.to_protocol_address_string())
    }

    // ── Participant node creation ────────────────────────────────────────

    /// Create encrypted participant `<to>` nodes for recipient JIDs.
    /// Returns `{ nodes: [...], shouldIncludeDeviceIdentity: boolean }`.
    /// Use `encodeProto('Message', obj)` on the JS side to produce the bytes.
    #[wasm_bindgen(js_name = createParticipantNodesBytes)]
    pub async fn create_participant_nodes_bytes(
        &self,
        jids: Vec<String>,
        bytes: &[u8],
        _extra_attrs: JsValue,
    ) -> Result<JsValue, JsError> {
        use prost::Message;
        let recipient_jids: Vec<wacore_binary::jid::Jid> = jids
            .iter()
            .map(|j| parse_jid(j))
            .collect::<Result<_, _>>()?;

        let msg = waproto::whatsapp::Message::decode(bytes)
            .map_err(|e| JsError::new(&format!("invalid message bytes: {e}")))?;

        let (nodes, should_include_device_identity) = self
            .client
            .signal()
            .create_participant_nodes(&recipient_jids, &msg)
            .await
            .map_err(js_err)?;

        let obj = js_sys::Object::new();
        let nodes_js = nodes_to_js_array(&nodes)
            .map_err(|e| JsError::new(&format!("node serialization failed: {e:?}")))?;
        js_sys::Reflect::set(&obj, &"nodes".into(), &nodes_js)
            .map_err(|e| JsError::new(&format!("{e:?}")))?;
        js_sys::Reflect::set(
            &obj,
            &"shouldIncludeDeviceIdentity".into(),
            &should_include_device_identity.into(),
        )
        .map_err(|e| JsError::new(&format!("{e:?}")))?;
        Ok(obj.into())
    }

    // ── Raw transport ────────────────────────────────────────────────────

    /// Send pre-marshaled bytes through the noise socket.
    #[wasm_bindgen(js_name = sendRawMessage)]
    pub async fn send_raw_message(&self, data: &[u8]) -> Result<(), JsError> {
        self.client
            .send_raw_bytes(data.to_vec())
            .await
            .map_err(js_err)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert GroupMetadata to a typed result struct.
fn group_metadata_to_result(
    metadata: &whatsapp_rust::features::GroupMetadata,
) -> crate::result_types::GroupMetadataResult {
    use crate::result_types::{GroupMetadataParticipant, GroupMetadataResult};
    GroupMetadataResult {
        id: metadata.id.to_string(),
        subject: metadata.subject.to_string(),
        participants: metadata
            .participants
            .iter()
            .map(|p| GroupMetadataParticipant {
                jid: p.jid.to_string(),
                phone_number: p.phone_number.as_ref().map(|pn| pn.to_string()),
                is_admin: p.is_admin(),
            })
            .collect(),
        addressing_mode: serde_json::to_string(&metadata.addressing_mode)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string(),
        creator: metadata.creator.as_ref().map(|j| j.to_string()),
        creation_time: metadata.creation_time.map(|v| v as f64),
        subject_time: metadata.subject_time.map(|v| v as f64),
        subject_owner: metadata.subject_owner.as_ref().map(|j| j.to_string()),
        description: metadata.description.clone(),
        description_id: metadata.description_id.clone(),
        is_locked: metadata.is_locked,
        is_announcement: metadata.is_announcement,
        ephemeral_expiration: metadata.ephemeral_expiration as f64,
        membership_approval: metadata.membership_approval,
        member_add_mode: metadata
            .member_add_mode
            .as_ref()
            .map(|m| format!("{:?}", m)),
        member_link_mode: metadata
            .member_link_mode
            .as_ref()
            .map(|m| format!("{:?}", m)),
        size: metadata.size.map(|v| v as f64),
        is_parent_group: metadata.is_parent_group,
        parent_group_jid: metadata.parent_group_jid.as_ref().map(|j| j.to_string()),
        is_default_sub_group: metadata.is_default_sub_group,
        is_general_chat: metadata.is_general_chat,
        allow_non_admin_sub_group_creation: metadata.allow_non_admin_sub_group_creation,
    }
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
) -> Result<Vec<String>, JsError> {
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

    let mut result = Vec::new();
    for hash in &selected_hashes {
        if let Ok(hash_arr) = <[u8; 32]>::try_from(hash.as_slice())
            && let Some((_, name)) = option_map.iter().find(|(h, _)| *h == hash_arr)
        {
            result.push(name.to_string());
        }
    }
    Ok(result)
}

/// Aggregate all votes for a poll. Returns `[{ name: string, voters: string[] }]`.
#[wasm_bindgen(js_name = getAggregateVotesInPollMessage)]
pub fn get_aggregate_votes_in_poll_message(
    option_names: Vec<String>,
    voters: Vec<crate::result_types::PollVoterEntry>,
    message_secret: &[u8],
    poll_msg_id: &str,
    poll_creator_jid: &str,
) -> Result<Vec<crate::result_types::PollAggregateResult>, JsError> {
    let creator = parse_jid(poll_creator_jid)?;

    let vote_data: Vec<(Jid, Vec<u8>, Vec<u8>)> = voters
        .into_iter()
        .map(|v| {
            let jid: Jid = v.voter.parse().map_err(js_err)?;
            Ok((jid, v.enc_payload, v.enc_iv))
        })
        .collect::<Result<_, JsError>>()?;

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

    Ok(results
        .into_iter()
        .map(|r| crate::result_types::PollAggregateResult {
            name: r.name,
            voters: r.voters,
        })
        .collect())
}

/// Convert any error to a proper JS Error object.
fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// Parse a JID string, returning a JS error on failure.
fn parse_jid(jid: &str) -> Result<Jid, JsError> {
    jid.parse().map_err(js_err)
}

// ---------------------------------------------------------------------------
// Node ↔ BinaryNode conversion
// ---------------------------------------------------------------------------
// Upstream Baileys uses `BinaryNode = { tag: string, attrs: Record<string, string>, content?: ... }`
// but wacore's `Node` has `Attrs(Vec<(Cow, NodeValue)>)` with tagged enum content.
// These converters bridge the two representations.

/// Convert a wacore `Node` → JS `BinaryNode` object `{ tag, attrs, content }`.
fn node_to_js(node: &wacore_binary::node::Node) -> Result<JsValue, JsValue> {
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"tag".into(), &node.tag.as_ref().into())?;

    // attrs: Record<string, string> — flatten Vec<(key, NodeValue)> to object
    let attrs_obj = js_sys::Object::new();
    for (key, value) in node.attrs.iter() {
        let val_str: String = value.as_str().into_owned();
        js_sys::Reflect::set(&attrs_obj, &key.as_ref().into(), &val_str.into())?;
    }
    js_sys::Reflect::set(&obj, &"attrs".into(), &attrs_obj.into())?;

    // content: BinaryNode[] | string | Uint8Array | undefined
    if let Some(content) = &node.content {
        let content_js = match content {
            wacore_binary::node::NodeContent::Nodes(nodes) => {
                let arr = js_sys::Array::new_with_length(nodes.len() as u32);
                for (i, child) in nodes.iter().enumerate() {
                    arr.set(i as u32, node_to_js(child)?);
                }
                arr.into()
            }
            wacore_binary::node::NodeContent::Bytes(bytes) => {
                js_sys::Uint8Array::from(bytes.as_slice()).into()
            }
            wacore_binary::node::NodeContent::String(s) => JsValue::from_str(s),
        };
        js_sys::Reflect::set(&obj, &"content".into(), &content_js)?;
    }

    Ok(obj.into())
}

/// Convert a yoke-borrowed `NodeRef` → JS `BinaryNode` object `{ tag, attrs, content }`.
/// Zero-copy: reads directly from the decoded buffer without cloning to `Node`.
fn node_ref_to_js(node: &wacore_binary::node::NodeRef<'_>) -> Result<JsValue, JsValue> {
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"tag".into(), &JsValue::from_str(&node.tag))?;

    let attrs_obj = js_sys::Object::new();
    for (key, value) in node.attrs_iter() {
        let js_val = match value {
            wacore_binary::node::ValueRef::String(s) => JsValue::from_str(s),
            wacore_binary::node::ValueRef::Jid(j) => {
                let mut s = String::with_capacity(j.user.len() + 20);
                wacore_binary::push_jid_to_string(&j.user, j.server, j.agent, j.device, &mut s);
                JsValue::from_str(&s)
            }
        };
        js_sys::Reflect::set(&attrs_obj, &JsValue::from_str(key), &js_val)?;
    }
    js_sys::Reflect::set(&obj, &"attrs".into(), &attrs_obj.into())?;

    if let Some(content) = node.content.as_deref() {
        let content_js = match content {
            wacore_binary::node::NodeContentRef::Nodes(children) => {
                let arr = js_sys::Array::new_with_length(children.len() as u32);
                for (i, child) in children.iter().enumerate() {
                    arr.set(i as u32, node_ref_to_js(child)?);
                }
                arr.into()
            }
            wacore_binary::node::NodeContentRef::Bytes(bytes) => {
                js_sys::Uint8Array::from(bytes.as_ref()).into()
            }
            wacore_binary::node::NodeContentRef::String(s) => JsValue::from_str(s),
        };
        js_sys::Reflect::set(&obj, &"content".into(), &content_js)?;
    }

    Ok(obj.into())
}

/// Convert a JS `BinaryNode` object → wacore `Node`.
fn js_to_node(val: &JsValue) -> Result<wacore_binary::node::Node, JsError> {
    use std::borrow::Cow;
    use wacore_binary::node::{Attrs, Node, NodeContent, NodeValue};

    let tag: String = js_sys::Reflect::get(val, &"tag".into())
        .map_err(|e| JsError::new(&format!("missing tag: {e:?}")))?
        .as_string()
        .ok_or_else(|| JsError::new("tag must be a string"))?;

    // Parse attrs: Record<string, string> → Vec<(Cow, NodeValue)>
    let attrs_val = js_sys::Reflect::get(val, &"attrs".into()).unwrap_or(JsValue::UNDEFINED);
    let mut attrs = Attrs::new();
    if attrs_val.is_object() && !attrs_val.is_undefined() && !attrs_val.is_null() {
        let keys = js_sys::Object::keys(&js_sys::Object::from(attrs_val.clone()));
        for i in 0..keys.length() {
            let key = keys.get(i).as_string().unwrap_or_default();
            let value = js_sys::Reflect::get(&attrs_val, &key.as_str().into())
                .unwrap_or(JsValue::UNDEFINED);
            let val_str = value.as_string().unwrap_or_default();
            // Try to parse as JID if it contains '@'
            if val_str.contains('@')
                && let Ok(jid) = val_str.parse::<wacore_binary::jid::Jid>()
            {
                attrs.push(Cow::Owned(key), NodeValue::Jid(jid));
                continue;
            }
            attrs.push(Cow::Owned(key), NodeValue::String(val_str.into()));
        }
    }

    // Parse content
    let content_val = js_sys::Reflect::get(val, &"content".into()).unwrap_or(JsValue::UNDEFINED);
    let content = if content_val.is_undefined() || content_val.is_null() {
        None
    } else if content_val.is_string() {
        Some(NodeContent::String(
            content_val.as_string().unwrap_or_default().into(),
        ))
    } else if content_val.is_instance_of::<js_sys::Uint8Array>() {
        let arr = js_sys::Uint8Array::from(content_val);
        Some(NodeContent::Bytes(arr.to_vec()))
    } else if js_sys::Array::is_array(&content_val) {
        let arr = js_sys::Array::from(&content_val);
        let mut children = Vec::with_capacity(arr.length() as usize);
        for i in 0..arr.length() {
            children.push(js_to_node(&arr.get(i))?);
        }
        Some(NodeContent::Nodes(children))
    } else {
        None
    };

    Ok(Node::new(Cow::Owned(tag), attrs, content))
}

/// Convert an array of wacore Nodes to JS BinaryNode array.
fn nodes_to_js_array(nodes: &[wacore_binary::node::Node]) -> Result<JsValue, JsValue> {
    let arr = js_sys::Array::new_with_length(nodes.len() as u32);
    for (i, node) in nodes.iter().enumerate() {
        arr.set(i as u32, node_to_js(node)?);
    }
    Ok(arr.into())
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

/// Consume a JS ReadableStream and upload via the HTTP client.
///
/// Buffers the stream into memory because `HttpClient::execute` takes `Vec<u8>`.
async fn stream_upload_via_js(
    client: &whatsapp_rust::Client,
    url: &str,
    body_stream: JsValue,
) -> Result<wacore::net::HttpResponse, JsValue> {
    use futures::StreamExt;

    let rs = wasm_streams::ReadableStream::from_raw(body_stream.unchecked_into());
    let mut stream = rs.into_stream();

    let mut body_bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| JsValue::from_str(&format!("stream read error: {e:?}")))?;
        let arr = js_sys::Uint8Array::new(&chunk);
        let start = body_bytes.len();
        body_bytes.resize(start + arr.length() as usize, 0);
        arr.copy_to(&mut body_bytes[start..]);
    }

    let request = wacore::net::HttpRequest::post(url.to_string())
        .with_header("Content-Type", "application/octet-stream")
        .with_header("Origin", "https://web.whatsapp.com")
        .with_body(body_bytes);

    client
        .http_client
        .execute(request)
        .await
        .map_err(|e| JsValue::from(js_err(e)))
}

fn parse_jid_and_msg_bytes(
    jid: &str,
    bytes: &[u8],
) -> Result<(Jid, waproto::whatsapp::Message), JsError> {
    use prost::Message;
    let to = parse_jid(jid)?;
    let msg = waproto::whatsapp::Message::decode(bytes)
        .map_err(|e| JsError::new(&format!("invalid message bytes: {e}")))?;
    Ok((to, msg))
}

/// Convert NewsletterMetadata to a typed result struct.
fn newsletter_metadata_to_result(
    meta: &whatsapp_rust::features::NewsletterMetadata,
) -> crate::result_types::NewsletterMetadataResult {
    crate::result_types::NewsletterMetadataResult {
        jid: meta.jid.to_string(),
        name: meta.name.to_string(),
        description: meta.description.clone(),
        subscriber_count: meta.subscriber_count as f64,
        verification: format!("{:?}", meta.verification),
        state: format!("{:?}", meta.state),
        picture_url: meta.picture_url.clone(),
        preview_url: meta.preview_url.clone(),
        invite_code: meta.invite_code.clone(),
        role: meta.role.as_ref().map(|r| format!("{:?}", r)),
        creation_time: meta.creation_time.map(|v| v as f64),
    }
}

fn business_profile_to_result(
    p: &wacore::iq::business::BusinessProfile,
) -> crate::result_types::BusinessProfileResult {
    crate::result_types::BusinessProfileResult {
        wid: p.wid.as_ref().map(|j| j.to_string()),
        description: p.description.clone(),
        email: p.email.clone(),
        website: p.website.clone(),
        categories: p
            .categories
            .iter()
            .map(|c| crate::result_types::BusinessCategoryResult {
                id: c.id.clone(),
                name: c.name.clone(),
            })
            .collect(),
        address: p.address.clone(),
        business_hours: crate::result_types::BusinessHoursResult {
            timezone: p.business_hours.timezone.clone(),
            business_config: p.business_hours.business_config.as_ref().map(|configs| {
                configs
                    .iter()
                    .map(|c| crate::result_types::BusinessHoursConfigResult {
                        day_of_week: c.day_of_week.to_string(),
                        mode: c.mode.to_string(),
                        open_time: c.open_time as f64,
                        close_time: c.close_time as f64,
                    })
                    .collect()
            }),
        },
    }
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
    )?;
    apply_cache_entry(
        js,
        "deviceRegistry",
        &mut config.device_registry_cache,
        &mut config.cache_stores.device_registry_cache,
    )?;
    apply_cache_entry(
        js,
        "lidPn",
        &mut config.lid_pn_cache,
        &mut config.cache_stores.lid_pn_cache,
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
