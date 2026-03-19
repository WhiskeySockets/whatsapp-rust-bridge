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

crate::wasm_send_sync!(WasmRuntime);

#[async_trait(?Send)]
impl Runtime for WasmRuntime {
    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>) -> AbortHandle {
        let (abort_handle, abort_reg) = futures::future::AbortHandle::new_pair();
        let abortable = futures::future::Abortable::new(future, abort_reg);
        wasm_bindgen_futures::spawn_local(async move {
            let _ = abortable.await;
        });
        wacore::runtime::AbortHandle::new(move || abort_handle.abort())
    }

    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()>>> {
        let ms = duration.as_millis().min(i32::MAX as u128) as i32;
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            let global = js_sys::global();
            if let Ok(set_timeout) = js_sys::Reflect::get(&global, &"setTimeout".into())
                && let Ok(set_timeout_fn) = set_timeout.dyn_into::<js_sys::Function>()
            {
                let _ = set_timeout_fn.call2(&JsValue::NULL, &resolve, &JsValue::from(ms));
                return;
            }
            // Fallback: resolve immediately if setTimeout unavailable
            let _ = resolve.call0(&JsValue::NULL);
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
