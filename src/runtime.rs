use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use wacore::runtime::{AbortHandle, Runtime};
use wasm_bindgen::prelude::*;

/// WASM-based implementation of [`Runtime`].
///
/// - `spawn` schedules tasks via `wasm_bindgen_futures::spawn_local`
/// - `sleep` uses `setTimeout(ms)`
/// - `yield_now` yields to the JS event loop via `setTimeout(0)` (macrotask)
/// - `spawn_blocking` runs inline (WASM is single-threaded, no thread pool)
///
/// Cooperative yielding is handled at the call-site (e.g., whatsapp-rust's
/// frame processing loop calls `yield_now()` every N frames) rather than
/// adding a blanket delay to every `spawn`. This avoids unnecessary latency
/// for tasks that don't need yielding while still preventing event loop
/// starvation during heavy processing.
pub struct WasmRuntime;

crate::wasm_send_sync!(WasmRuntime);

/// Yield to the JS event loop via `setTimeout(0)`.
///
/// Unlike `Promise.resolve()` (microtask, same tick), `setTimeout(0)` creates
/// a macrotask that runs in the NEXT event loop tick. This lets pending I/O
/// (WebSocket data, storage callbacks) and other scheduled work run before
/// the current task resumes.
fn set_timeout_0() -> Pin<Box<dyn Future<Output = ()>>> {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let global = js_sys::global();
        if let Ok(set_timeout) = js_sys::Reflect::get(&global, &"setTimeout".into())
            && let Ok(set_timeout_fn) = set_timeout.dyn_into::<js_sys::Function>()
        {
            let _ = set_timeout_fn.call2(&JsValue::NULL, &resolve, &JsValue::from(0));
            return;
        }
        // Fallback: resolve immediately (environments without setTimeout)
        let _ = resolve.call0(&JsValue::NULL);
    });
    let js_future = wasm_bindgen_futures::JsFuture::from(promise);
    Box::pin(async move {
        let _ = js_future.await;
    })
}

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

    fn yield_now(&self) -> Option<Pin<Box<dyn Future<Output = ()>>>> {
        Some(set_timeout_0())
    }
}
