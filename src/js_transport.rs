//! JS WebSocket transport adapter.
//!
//! Uses raw `js_sys::Function` callbacks instead of wasm-bindgen extern types
//! to avoid reentrancy panics. wasm-bindgen's extern types use an internal
//! object slab with RefCell-like borrowing that panics on reentrant calls
//! (e.g. disconnect → ws.close → ws.onclose → reconnect → connect).

use std::sync::Arc;

use async_channel::Receiver;
use async_trait::async_trait;
use bytes::Bytes;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use wacore::net::{Transport, TransportEvent, TransportFactory};

// ---------------------------------------------------------------------------
// TypeScript interface (documentation only — actual impl uses raw Functions)
// ---------------------------------------------------------------------------

#[wasm_bindgen(typescript_custom_section)]
const TS_TRANSPORT: &str = r#"
/**
 * JS transport callbacks for WebSocket management.
 *
 * Passed to `createWhatsAppClient` as the transport config.
 *
 * `connect(handle)` is called when the client needs a connection:
 *   - Create a WebSocket
 *   - Wire ws.onopen → handle.onConnected()
 *   - Wire ws.onmessage → handle.onData(data)
 *   - Wire ws.onclose → handle.onDisconnected()
 *
 * `send(data)` sends raw bytes over the active WebSocket.
 * `disconnect()` closes the WebSocket.
 */
export interface JsTransportHandle {
    onConnected(): void;
    onData(data: Uint8Array): void;
    onDisconnected(): void;
}

export interface JsTransportCallbacks {
    connect(handle: JsTransportHandle): void | Promise<void>;
    send(data: Uint8Array): void | Promise<void>;
    disconnect(): void | Promise<void>;
}
"#;

// ---------------------------------------------------------------------------
// Transport handle — JS pushes WebSocket events through this
// ---------------------------------------------------------------------------

/// Handle given to JS `connect()` for pushing WebSocket events into Rust.
/// Create a JS handle object with onConnected/onData/onDisconnected closures.
///
/// Returns a plain JS object (not a wasm-bindgen struct) to avoid reentrancy.
/// Closures push events into the async_channel without entering WASM.
fn create_js_handle(event_tx: async_channel::Sender<TransportEvent>) -> JsValue {
    let obj = js_sys::Object::new();

    let tx = event_tx.clone();
    let on_data = Closure::wrap(Box::new(move |data: js_sys::Uint8Array| {
        let bytes = data.to_vec();
        match tx.try_send(TransportEvent::DataReceived(Bytes::from(bytes))) {
            Ok(()) => {}
            Err(async_channel::TrySendError::Closed(_)) => {
                log::debug!("Transport channel closed, data event dropped (shutdown in progress)");
            }
            Err(async_channel::TrySendError::Full(_)) => {
                // Dropping a frame desyncs the noise counter; force reconnect
                // instead — WA redelivers unacked messages on resume.
                log::error!("Transport channel full; closing to force reconnect");
                tx.close();
            }
        }
    }) as Box<dyn FnMut(js_sys::Uint8Array)>);
    let _ = js_sys::Reflect::set(&obj, &"onData".into(), &on_data.into_js_value());

    let tx = event_tx.clone();
    let on_connected =
        Closure::wrap(
            Box::new(move || match tx.try_send(TransportEvent::Connected) {
                Ok(()) => {}
                Err(async_channel::TrySendError::Closed(_)) => {
                    log::debug!("Transport channel closed, Connected event dropped");
                }
                Err(async_channel::TrySendError::Full(_)) => {
                    log::error!("Transport channel full on Connected; closing");
                    tx.close();
                }
            }) as Box<dyn FnMut()>,
        );
    let _ = js_sys::Reflect::set(&obj, &"onConnected".into(), &on_connected.into_js_value());

    let tx = event_tx;
    let on_disconnected =
        Closure::wrap(
            Box::new(move || match tx.try_send(TransportEvent::Disconnected) {
                Ok(()) => {}
                Err(async_channel::TrySendError::Closed(_)) => {
                    log::debug!("Transport channel closed, Disconnected event dropped");
                }
                Err(async_channel::TrySendError::Full(_)) => {
                    // Receiver treats channel close as Disconnected (client.rs recv Err).
                    log::error!("Transport channel full on Disconnected; closing");
                    tx.close();
                }
            }) as Box<dyn FnMut()>,
        );
    let _ = js_sys::Reflect::set(
        &obj,
        &"onDisconnected".into(),
        &on_disconnected.into_js_value(),
    );

    obj.into()
}

// ---------------------------------------------------------------------------
// Internal: raw JS function storage (avoids wasm-bindgen reentrancy)
// ---------------------------------------------------------------------------

/// Stores the transport callbacks as raw JS functions.
///
/// Using `js_sys::Function` instead of wasm-bindgen extern types avoids the
/// "recursive use of an object" panic that occurs when a callback reentrantly
/// calls back into WASM (e.g. disconnect → ws.close → reconnect → connect).
struct RawTransportCallbacks {
    connect_fn: js_sys::Function,
    send_fn: js_sys::Function,
    disconnect_fn: js_sys::Function,
    /// The original JS object — kept alive to prevent GC
    _js_obj: JsValue,
}

crate::wasm_send_sync!(RawTransportCallbacks);

impl RawTransportCallbacks {
    /// Extract callbacks from a JS object with connect/send/disconnect methods.
    fn from_js(obj: JsValue) -> Result<Self, JsValue> {
        let connect_fn = js_sys::Reflect::get(&obj, &"connect".into())?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| JsValue::from_str("transport.connect must be a function"))?;
        let send_fn = js_sys::Reflect::get(&obj, &"send".into())?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| JsValue::from_str("transport.send must be a function"))?;
        let disconnect_fn = js_sys::Reflect::get(&obj, &"disconnect".into())?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| JsValue::from_str("transport.disconnect must be a function"))?;

        Ok(Self {
            connect_fn,
            send_fn,
            disconnect_fn,
            _js_obj: obj,
        })
    }

    async fn call_connect_js(&self, handle: JsValue) -> Result<(), anyhow::Error> {
        let result = self
            .connect_fn
            .call1(&JsValue::NULL, &handle)
            .map_err(|e| anyhow::anyhow!("connect: {e:?}"))?;
        resolve_maybe(result).await
    }

    async fn call_send(&self, data: &[u8]) -> Result<(), anyhow::Error> {
        let uint8 = js_sys::Uint8Array::from(data);
        let result = self
            .send_fn
            .call1(&JsValue::NULL, &uint8.into())
            .map_err(|e| anyhow::anyhow!("send: {e:?}"))?;
        resolve_maybe(result).await
    }

    async fn call_disconnect(&self) -> Result<(), anyhow::Error> {
        let result = self
            .disconnect_fn
            .call0(&JsValue::NULL)
            .map_err(|e| anyhow::anyhow!("disconnect: {e:?}"))?;
        resolve_maybe(result).await
    }
}

async fn resolve_maybe(val: JsValue) -> Result<(), anyhow::Error> {
    if val.is_instance_of::<js_sys::Promise>() {
        let promise = js_sys::Promise::unchecked_from_js(val);
        let future: JsFuture = JsFuture::from(promise);
        let _result: JsValue = future.await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Transport implementation
// ---------------------------------------------------------------------------

struct JsTransportInner {
    callbacks: Arc<RawTransportCallbacks>,
}

crate::wasm_send_sync!(JsTransportInner);

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Transport for JsTransportInner {
    async fn send(&self, data: Bytes) -> Result<(), anyhow::Error> {
        log::trace!("Transport::send {} bytes", data.len());
        self.callbacks.call_send(&data).await
    }

    async fn disconnect(&self) {
        log::debug!("Transport::disconnect called");
        let _ = self.callbacks.call_disconnect().await;
        log::debug!("Transport::disconnect completed");
    }
}

// ---------------------------------------------------------------------------
// TransportFactory
// ---------------------------------------------------------------------------

pub struct JsTransportFactory {
    callbacks: Arc<RawTransportCallbacks>,
}

crate::wasm_send_sync!(JsTransportFactory);

impl JsTransportFactory {
    pub fn from_js(obj: JsValue) -> Result<Self, JsValue> {
        Ok(Self {
            callbacks: Arc::new(RawTransportCallbacks::from_js(obj)?),
        })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl TransportFactory for JsTransportFactory {
    async fn create_transport(
        &self,
    ) -> Result<(Arc<dyn Transport>, Receiver<TransportEvent>), anyhow::Error> {
        log::debug!("JsTransportFactory::create_transport called");
        // Sized for burst absorption; overflow is a real lag signal, not noise.
        let (event_tx, event_rx) = async_channel::bounded(32768);
        let handle = create_js_handle(event_tx);

        log::debug!("Calling JS connect(handle)...");
        self.callbacks.call_connect_js(handle).await?;
        log::debug!("JS connect(handle) returned successfully");

        let transport = Arc::new(JsTransportInner {
            callbacks: self.callbacks.clone(),
        }) as Arc<dyn Transport>;

        Ok((transport, event_rx))
    }
}
