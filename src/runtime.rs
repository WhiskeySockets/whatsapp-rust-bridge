use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use wacore::runtime::{AbortHandle, Runtime};
use wasm_bindgen::prelude::*;

/// WASM-based implementation of [`Runtime`].
///
/// - `spawn` uses `wasm_bindgen_futures::spawn_local` (single-threaded)
/// - `sleep` uses `js_sys::Promise` + `setTimeout`
/// - `spawn_blocking` runs inline (WASM is single-threaded, no thread pool)
pub struct WasmRuntime;

// SAFETY: WASM is single-threaded — Send + Sync are trivially satisfied.
unsafe impl Send for WasmRuntime {}
unsafe impl Sync for WasmRuntime {}

#[async_trait(?Send)]
impl Runtime for WasmRuntime {
    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>) -> AbortHandle {
        wasm_bindgen_futures::spawn_local(async move {
            future.await;
        });
        AbortHandle::noop()
    }

    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()>>> {
        let ms = duration.as_millis() as i32;
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            let global = js_sys::global();
            if let Ok(set_timeout) = js_sys::Reflect::get(&global, &"setTimeout".into()) {
                if let Ok(set_timeout_fn) = set_timeout.dyn_into::<js_sys::Function>() {
                    let _ = set_timeout_fn.call2(&JsValue::NULL, &resolve, &JsValue::from(ms));
                }
            }
        });
        let js_future = wasm_bindgen_futures::JsFuture::from(promise);
        Box::pin(async move {
            let _ = js_future.await;
        })
    }

    fn spawn_blocking(&self, f: Box<dyn FnOnce() + 'static>) -> Pin<Box<dyn Future<Output = ()>>> {
        Box::pin(async move {
            f();
        })
    }
}
