use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use async_trait::async_trait;
use wacore::runtime::{AbortHandle, Runtime};
use wasm_bindgen::prelude::*;

/// WASM-based implementation of [`Runtime`].
///
/// - `spawn` schedules tasks via `wasm_bindgen_futures::spawn_local`
/// - `sleep` uses `setTimeout(ms)`
/// - `yield_now` yields via `MessageChannel` (zero-allocation macrotask)
/// - `spawn_blocking` runs inline (WASM is single-threaded, no thread pool)
pub struct WasmRuntime;

crate::wasm_send_sync!(WasmRuntime);

/// Cached reference to `globalThis.setTimeout`.
static SET_TIMEOUT_FN: std::sync::OnceLock<JsValue> = std::sync::OnceLock::new();

fn get_set_timeout() -> &'static JsValue {
    SET_TIMEOUT_FN.get_or_init(|| {
        let global = js_sys::global();
        js_sys::Reflect::get(&global, &"setTimeout".into()).unwrap_or(JsValue::UNDEFINED)
    })
}

// ---------------------------------------------------------------------------
// MessageChannel-based yielding
// ---------------------------------------------------------------------------
//
// Instead of creating a new Promise + setTimeout per yield, we use a single
// MessageChannel. `port1.postMessage()` schedules a macrotask that fires
// `port2.onmessage`, which wakes the next pending waker. This eliminates:
// - Promise allocation per yield
// - setTimeout timer object per yield
// - Closure allocation per yield
// - JsFuture global handle per yield
//
// Cost per yield: one `postMessage(null)` call + one `Waker` (Rust-only, no JS).

thread_local! {
    static YIELD_WAKERS: RefCell<VecDeque<Waker>> = const { RefCell::new(VecDeque::new()) };
    static MSG_POST_FN: RefCell<Option<(js_sys::Function, js_sys::Object)>> = const { RefCell::new(None) };
}

/// Initialize the MessageChannel yield mechanism. Called once at startup.
fn ensure_msg_channel() {
    MSG_POST_FN.with(|cached| {
        if cached.borrow().is_some() {
            return;
        }

        let global = js_sys::global();

        let channel = js_sys::Reflect::get(&global, &"MessageChannel".into())
            .ok()
            .and_then(|mc| mc.dyn_into::<js_sys::Function>().ok())
            .and_then(|ctor| js_sys::Reflect::construct(&ctor, &js_sys::Array::new()).ok());

        let Some(channel) = channel else {
            return;
        };

        let port1 = js_sys::Reflect::get(&channel, &"port1".into())
            .ok()
            .and_then(|p| p.dyn_into::<js_sys::Object>().ok());
        let port2 = js_sys::Reflect::get(&channel, &"port2".into()).ok();

        if let (Some(p1), Some(p2)) = (port1, port2) {
            // port2.onmessage wakes the next pending yielder
            let callback = Closure::wrap(Box::new(|| {
                YIELD_WAKERS.with(|wakers| {
                    if let Some(waker) = wakers.borrow_mut().pop_front() {
                        waker.wake();
                    }
                });
            }) as Box<dyn FnMut()>);

            let _ =
                js_sys::Reflect::set(&p2, &"onmessage".into(), callback.as_ref().unchecked_ref());
            callback.forget();

            // Cache port1.postMessage as a bound function
            let post_fn = js_sys::Reflect::get(&p1, &"postMessage".into())
                .ok()
                .and_then(|f| f.dyn_into::<js_sys::Function>().ok());

            if let Some(post_fn) = post_fn {
                *cached.borrow_mut() = Some((post_fn, p1));
            }
        }
    });
}

/// Zero-allocation yield to the JS event loop via MessageChannel.
///
/// `port1.postMessage(null)` schedules a macrotask on `port2.onmessage`
/// which wakes this future. No Promise, no setTimeout, no closure per call.
pub(crate) fn set_timeout_0() -> Pin<Box<dyn Future<Output = ()>>> {
    ensure_msg_channel();

    // Try MessageChannel path
    let has_channel = MSG_POST_FN.with(|cached| cached.borrow().is_some());

    if has_channel {
        Box::pin(MsgChannelYield { registered: false })
    } else {
        // Fallback: setTimeout(0) for environments without MessageChannel
        set_timeout_yield()
    }
}

/// Future that resolves when the MessageChannel fires.
struct MsgChannelYield {
    registered: bool,
}

impl Future for MsgChannelYield {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.registered {
            // Woken by onmessage callback — we're done
            Poll::Ready(())
        } else {
            // First poll: register waker and post message
            self.registered = true;
            YIELD_WAKERS.with(|wakers| {
                wakers.borrow_mut().push_back(cx.waker().clone());
            });
            MSG_POST_FN.with(|cached| {
                if let Some((ref post_fn, ref port1)) = *cached.borrow() {
                    let _ = post_fn.call1(port1, &JsValue::NULL);
                }
            });
            Poll::Pending
        }
    }
}

/// Fallback: yield via setTimeout(0) — used when MessageChannel is unavailable.
fn set_timeout_yield() -> Pin<Box<dyn Future<Output = ()>>> {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let st = get_set_timeout();
        if let Ok(set_timeout_fn) = st.clone().dyn_into::<js_sys::Function>() {
            let _ = set_timeout_fn.call2(&JsValue::NULL, &resolve, &JsValue::from(0));
        } else {
            let _ = resolve.call0(&JsValue::NULL);
        }
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
            let st = get_set_timeout();
            if let Ok(set_timeout_fn) = st.clone().dyn_into::<js_sys::Function>() {
                let _ = set_timeout_fn.call2(&JsValue::NULL, &resolve, &JsValue::from(ms));
            } else {
                let _ = resolve.call0(&JsValue::NULL);
            }
        });
        let js_future = wasm_bindgen_futures::JsFuture::from(promise);
        Box::pin(async move {
            let _ = js_future.await;
        })
    }

    fn spawn_blocking(&self, f: Box<dyn FnOnce() + 'static>) -> Pin<Box<dyn Future<Output = ()>>> {
        // WASM is single-threaded: yield to event loop BEFORE running the
        // blocking closure so pending I/O (WebSocket, storage) can complete.
        // Then run the closure, then yield again to let results propagate.
        Box::pin(async move {
            set_timeout_0().await; // yield before — let I/O callbacks run
            f();
            set_timeout_0().await; // yield after — let event loop process results
        })
    }

    fn yield_now(&self) -> Option<Pin<Box<dyn Future<Output = ()>>>> {
        // Always yield in WASM — the single-threaded event loop needs every
        // opportunity to process pending I/O (WebSocket data, storage callbacks,
        // timer callbacks). The caller already throttles (e.g., every 10 frames),
        // but in WASM even that can be too infrequent during heavy offline processing.
        Some(set_timeout_0())
    }

    fn yield_frequency(&self) -> u32 {
        // Yield every single frame in WASM. The default (10) is too infrequent
        // for single-threaded execution — pending I/O starves between yields.
        1
    }
}
