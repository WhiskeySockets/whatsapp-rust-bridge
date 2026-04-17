use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use async_trait::async_trait;
use wacore::runtime::{AbortHandle, Runtime};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// WASM-based implementation of [`Runtime`].
///
/// - `spawn` schedules tasks via `wasm_bindgen_futures::spawn_local`
/// - `sleep` uses `setTimeout(ms)`
/// - `yield_now` yields via `MessageChannel` (zero-allocation macrotask)
/// - `spawn_blocking` runs inline (WASM is single-threaded, no thread pool)
pub struct WasmRuntime;

crate::wasm_send_sync!(WasmRuntime);

/// Cached reference to `globalThis.setTimeout`, resolved once and stored as
/// `js_sys::Function` so hot paths avoid a per-call `dyn_into` + clone.
static SET_TIMEOUT_FN: std::sync::OnceLock<Option<js_sys::Function>> = std::sync::OnceLock::new();
/// Cached reference to `globalThis.clearTimeout` — same rationale as above.
static CLEAR_TIMEOUT_FN: std::sync::OnceLock<Option<js_sys::Function>> = std::sync::OnceLock::new();

fn resolve_global_fn(name: &str) -> Option<js_sys::Function> {
    let global = js_sys::global();
    js_sys::Reflect::get(&global, &name.into())
        .ok()
        .and_then(|v| v.dyn_into::<js_sys::Function>().ok())
}

fn get_set_timeout() -> Option<&'static js_sys::Function> {
    SET_TIMEOUT_FN
        .get_or_init(|| resolve_global_fn("setTimeout"))
        .as_ref()
}

fn get_clear_timeout() -> Option<&'static js_sys::Function> {
    CLEAR_TIMEOUT_FN
        .get_or_init(|| resolve_global_fn("clearTimeout"))
        .as_ref()
}

/// Call `clearTimeout(id)` to cancel a pending timer. Silently no-ops if
/// `globalThis.clearTimeout` is unavailable or the call throws.
fn clear_timeout(id: &JsValue) {
    if let Some(clear_fn) = get_clear_timeout() {
        let _ = clear_fn.call1(&JsValue::NULL, id);
    }
}

// ---------------------------------------------------------------------------
// Cancellation-aware sleep future
// ---------------------------------------------------------------------------
//
// `Promise::new` + `setTimeout` alone leaks timers on cancellation: if the
// owning Rust future is dropped (via `futures::select!`, Abortable, etc.)
// before the timer fires, the Rust-side `JsFuture` goes away — but the
// JS-side `setTimeout` callback remains queued and keeps the Node.js event
// loop alive until it fires naturally. For long sleeps (minutes) during
// shutdown this causes the process to hang.
//
// `SleepFut` captures the timer ID returned by `setTimeout` into a shared
// `Rc<RefCell<Option<JsValue>>>` so that Drop can call `clearTimeout(id)`.
// When the timer fires naturally we clear the slot so Drop is a no-op.

struct SleepFut {
    /// Shared timer ID — `Some` while the timer is pending, cleared to
    /// `None` either when the timer fires (in `poll`) or when we've already
    /// cancelled it (in `drop`). When `setTimeout` is unavailable the slot
    /// stays `None` and the underlying promise is already resolved.
    timer_id: Rc<RefCell<Option<JsValue>>>,
    js_future: JsFuture,
}

impl Future for SleepFut {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match Pin::new(&mut self.js_future).poll(cx) {
            Poll::Ready(_) => {
                // Timer fired naturally (or was never scheduled) — clear the ID
                // so Drop is a no-op.
                self.timer_id.borrow_mut().take();
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for SleepFut {
    fn drop(&mut self) {
        if let Some(id) = self.timer_id.borrow_mut().take() {
            clear_timeout(&id);
        }
    }
}

/// Schedule `callback` via `setTimeout(ms)` and return the timer ID.
///
/// Returns `None` if scheduling failed (e.g. `globalThis.setTimeout` missing
/// — the caller should treat this as "resolved synchronously").
fn schedule_timeout(callback: &JsValue, ms: i32) -> Option<JsValue> {
    let set_timeout_fn = get_set_timeout()?;
    let id = set_timeout_fn
        .call2(&JsValue::NULL, callback, &JsValue::from(ms))
        .ok()?;
    if id.is_undefined() || id.is_null() {
        None
    } else {
        Some(id)
    }
}

/// Build a cancellation-aware sleep future.
///
/// `ms == 0` still allocates a timer to preserve "yield to event loop" semantics,
/// but Drop-cancellation works identically.
fn make_sleep(ms: i32) -> SleepFut {
    // `timer_id_slot` is populated by the Promise executor (runs synchronously
    // during `Promise::new`). We share it with `SleepFut` so Drop can cancel.
    let timer_id_slot: Rc<RefCell<Option<JsValue>>> = Rc::new(RefCell::new(None));

    let executor_slot = timer_id_slot.clone();
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        match schedule_timeout(&resolve, ms) {
            Some(id) => *executor_slot.borrow_mut() = Some(id),
            None => {
                // Couldn't schedule — resolve synchronously so the future is Ready.
                let _ = resolve.call0(&JsValue::NULL);
            }
        }
    });

    SleepFut {
        timer_id: timer_id_slot,
        js_future: JsFuture::from(promise),
    }
}

// ---------------------------------------------------------------------------
// setImmediate-based yielding
// ---------------------------------------------------------------------------
//
// Uses `setImmediate` for cooperative yielding. This is critical because
// `setImmediate` fires in the **check phase** of the Node.js event loop,
// which runs AFTER the **I/O poll phase**. This means:
// - File I/O callbacks (writeFile for session storage) complete before yield
// - Network I/O callbacks (WebSocket data) complete before yield
// - Timer callbacks complete before yield
//
// MessageChannel.postMessage was previously used but it fires as a macrotask
// that runs BEFORE I/O polling, causing deadlocks: WASM awaits a JS Promise
// that needs writeFile to complete, but writeFile needs the I/O poll phase,
// which never runs because MessageChannel macrotasks keep firing.
//
// setImmediate is available in Node.js and Bun (our targets). For browser
// environments, we fall back to setTimeout(0).
//
// Cost per yield: one `setImmediate(callback)` call + one `Waker`.
// Slightly more overhead than MessageChannel (closure allocation per call)
// but eliminates the I/O starvation deadlock.

thread_local! {
    static YIELD_WAKERS: RefCell<VecDeque<Waker>> = const { RefCell::new(VecDeque::new()) };
    static SET_IMMEDIATE_FN: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// Spawn throttle — staggers task starts to prevent microtask storms
// ---------------------------------------------------------------------------
//
// Problem: During offline sync, hundreds of per-chat workers are spawned via
// spawn_local. They all immediately contend on an upstream 1-permit semaphore.
// Each permit release wakes ALL waiters as microtasks (thundering herd). Only
// one wins; the rest re-pend. This microtask storm prevents JS callbacks
// (storage Promises, timers, WebSocket I/O) from running → 100% CPU freeze.
//
// Solution: A FIFO queue batches spawn requests. A single drain loop starts
// tasks in order, yielding to the timer queue (setTimeout) between batches.
// This ensures:
//   - First batch starts immediately (zero overhead in normal operation)
//   - Excess tasks wait in a Rust VecDeque (no JS allocations while waiting)
//   - setTimeout yields between batches let JS timers/Promises interleave
//   - Long-lived tasks (noise sender, per-chat workers) don't block the queue
//     because the queue tracks *starts*, not *lifetime*

/// Max tasks to start per batch before yielding to the JS timer queue.
const SPAWN_BATCH_SIZE: usize = 16;

type SpawnTask = Pin<Box<dyn Future<Output = ()> + 'static>>;

thread_local! {
    static SPAWN_QUEUE: RefCell<VecDeque<SpawnTask>> = const { RefCell::new(VecDeque::new()) };
    static SPAWN_DRAINING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Enqueue a future and ensure the drain loop is running.
fn enqueue_spawn(task: SpawnTask) {
    SPAWN_QUEUE.with(|q| q.borrow_mut().push_back(task));

    let already_draining = SPAWN_DRAINING.with(|d| {
        if d.get() {
            true
        } else {
            d.set(true);
            false
        }
    });

    if !already_draining {
        wasm_bindgen_futures::spawn_local(drain_spawn_queue());
    }
}

/// Drain loop: starts tasks in batches, yielding to the JS timer queue between
/// batches so setTimeout/setInterval/Promise callbacks can run.
async fn drain_spawn_queue() {
    loop {
        // Take up to SPAWN_BATCH_SIZE tasks from the queue
        let batch: Vec<SpawnTask> = SPAWN_QUEUE.with(|q| {
            let mut q = q.borrow_mut();
            let n = q.len().min(SPAWN_BATCH_SIZE);
            q.drain(..n).collect()
        });

        if batch.is_empty() {
            // Queue is empty — stop draining
            SPAWN_DRAINING.with(|d| d.set(false));
            return;
        }

        // Start all tasks in this batch (they become independent spawn_local futures)
        for task in batch {
            wasm_bindgen_futures::spawn_local(task);
        }

        // Yield to the JS timer queue between batches. Uses setTimeout(0) so
        // timer callbacks (setInterval, storage Promises) can interleave.
        // MessageChannel has higher priority than timers and would still starve.
        set_timeout_yield().await;

        // After yielding, check if more tasks were enqueued while we were waiting
    }
}

// Cached persistent waker-drain callback, reused for every setImmediate call.
thread_local! {
    static IMMEDIATE_CB: RefCell<Option<JsValue>> = const { RefCell::new(None) };
}

/// Cache `setImmediate` and create a persistent drain callback.
fn ensure_set_immediate() {
    SET_IMMEDIATE_FN.with(|cached| {
        if cached.borrow().is_some() {
            return;
        }
        let global = js_sys::global();
        if let Ok(si) = js_sys::Reflect::get(&global, &"setImmediate".into())
            && let Ok(f) = si.dyn_into::<js_sys::Function>()
        {
            *cached.borrow_mut() = Some(f);
        }
    });

    IMMEDIATE_CB.with(|cached| {
        if cached.borrow().is_some() {
            return;
        }
        // Create ONE persistent callback that wakes the next yielder.
        // This is called by setImmediate each time — same function, no allocation.
        let callback = Closure::wrap(Box::new(|| {
            YIELD_WAKERS.with(|wakers| {
                if let Some(waker) = wakers.borrow_mut().pop_front() {
                    waker.wake();
                }
            });
        }) as Box<dyn FnMut()>);
        *cached.borrow_mut() = Some(callback.into_js_value());
    });
}

/// Yield to the JS event loop. Uses `setImmediate` (Node.js/Bun) which fires
/// in the **check phase** — after I/O polling — so file/network I/O (writeFile,
/// WebSocket) can complete between yields. Falls back to `setTimeout(0)` in
/// environments without `setImmediate`.
///
/// Cost per yield: one `setImmediate(cachedCallback)` call + one `Waker` clone.
/// No Promise, no closure, no timer object allocated per call.
pub(crate) fn set_timeout_0() -> Pin<Box<dyn Future<Output = ()>>> {
    ensure_set_immediate();

    let has_immediate = SET_IMMEDIATE_FN.with(|cached| cached.borrow().is_some());

    if has_immediate {
        Box::pin(SetImmediateYield { registered: false })
    } else {
        set_timeout_yield()
    }
}

/// Future that resolves when `setImmediate` fires.
struct SetImmediateYield {
    registered: bool,
}

impl Future for SetImmediateYield {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.registered {
            Poll::Ready(())
        } else {
            self.registered = true;
            YIELD_WAKERS.with(|wakers| {
                wakers.borrow_mut().push_back(cx.waker().clone());
            });
            // Call setImmediate with the cached callback — fires after I/O poll
            SET_IMMEDIATE_FN.with(|si| {
                IMMEDIATE_CB.with(|cb| {
                    let si_ref = si.borrow();
                    let cb_ref = cb.borrow();
                    if let (Some(si_fn), Some(cb_val)) = (si_ref.as_ref(), cb_ref.as_ref()) {
                        let _ = si_fn.call1(&JsValue::NULL, cb_val);
                    }
                });
            });
            Poll::Pending
        }
    }
}

/// Fallback: yield via setTimeout(0) — used when `setImmediate` is unavailable.
///
/// Uses the cancellation-aware `SleepFut` so a dropped yield doesn't leak a
/// pending timer into the event loop. For a 0ms timer this rarely matters in
/// practice, but it's free insurance and keeps the code path consistent.
fn set_timeout_yield() -> Pin<Box<dyn Future<Output = ()>>> {
    Box::pin(make_sleep(0))
}

#[async_trait(?Send)]
impl Runtime for WasmRuntime {
    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>) -> AbortHandle {
        let (abort_handle, abort_reg) = futures::future::AbortHandle::new_pair();
        let abortable = futures::future::Abortable::new(future, abort_reg);
        enqueue_spawn(Box::pin(async move {
            let _ = abortable.await;
        }));
        wacore::runtime::AbortHandle::new(move || abort_handle.abort())
    }

    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()>>> {
        let ms = duration.as_millis().min(i32::MAX as u128) as i32;
        // `make_sleep` returns a `SleepFut` that calls `clearTimeout` on Drop
        // if the underlying timer hasn't fired yet — prevents leaking pending
        // timers into the Node.js event loop on cancellation/abort.
        Box::pin(make_sleep(ms))
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
