//! WASM time providers backed by JS globals.

use wacore::time::{MonotonicProvider, TimeProvider};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now, catch)]
    fn performance_now() -> Result<f64, JsValue>;
}

pub struct JsTimeProvider;

crate::wasm_send_sync!(JsTimeProvider);

impl TimeProvider for JsTimeProvider {
    fn now_millis(&self) -> i64 {
        js_sys::Date::now() as i64
    }
}

/// Sub-millisecond monotonic clock backed by `performance.now()`.
/// Available in browsers, Node 16+, and Bun. The spec guarantees the value
/// is monotonic non-decreasing within an agent.
pub struct JsMonotonicProvider;

crate::wasm_send_sync!(JsMonotonicProvider);

impl MonotonicProvider for JsMonotonicProvider {
    fn now_nanos(&self) -> u64 {
        match performance_now() {
            Ok(ms) if ms.is_finite() && ms >= 0.0 => (ms * 1_000_000.0) as u64,
            _ => 0,
        }
    }
}

/// Initialize the WASM time providers. Call once at startup.
pub fn init_time_provider() {
    let _ = wacore::time::set_time_provider(JsTimeProvider);
    let _ = wacore::time::set_monotonic_provider(JsMonotonicProvider);
}
