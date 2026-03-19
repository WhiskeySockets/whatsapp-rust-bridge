//! WASM time provider using JS `Date.now()`.

use wacore::time::TimeProvider;

pub struct JsTimeProvider;

// SAFETY: WASM is single-threaded.
unsafe impl Send for JsTimeProvider {}
unsafe impl Sync for JsTimeProvider {}

impl TimeProvider for JsTimeProvider {
    fn now_millis(&self) -> i64 {
        js_sys::Date::now() as i64
    }
}

/// Initialize the WASM time provider. Call once at startup.
pub fn init_time_provider() {
    let _ = wacore::time::set_time_provider(JsTimeProvider);
}
