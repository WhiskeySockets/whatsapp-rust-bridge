use js_sys::Object;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const LOGGER_TS: &str = r#"
export interface ILogger {
    level: string;
    trace(obj: object, msg?: string): void;
    debug(obj: object, msg?: string): void;
    info(obj: object, msg?: string): void;
    warn(obj: object, msg?: string): void;
    error(obj: object, msg?: string): void;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "ILogger")]
    #[derive(Clone)]
    pub type JsLogger;

    #[wasm_bindgen(structural, method, getter, js_name = level)]
    fn js_level(this: &JsLogger) -> String;

    #[wasm_bindgen(structural, method, js_name = trace)]
    fn js_trace(this: &JsLogger, obj: &Object, msg: Option<&str>);

    #[wasm_bindgen(structural, method, js_name = debug)]
    fn js_debug(this: &JsLogger, obj: &Object, msg: Option<&str>);

    #[wasm_bindgen(structural, method, js_name = info)]
    fn js_info(this: &JsLogger, obj: &Object, msg: Option<&str>);

    #[wasm_bindgen(structural, method, js_name = warn)]
    fn js_warn(this: &JsLogger, obj: &Object, msg: Option<&str>);

    #[wasm_bindgen(structural, method, js_name = error)]
    fn js_error(this: &JsLogger, obj: &Object, msg: Option<&str>);
}

thread_local! {
    static JS_LOGGER: RefCell<Option<JsLogger>> = const { RefCell::new(None) };
}

static LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);

fn parse_level_filter(level_str: &str) -> LevelFilter {
    match level_str.to_lowercase().as_str() {
        "trace" | "10" => LevelFilter::Trace,
        "debug" | "20" => LevelFilter::Debug,
        "info" | "30" => LevelFilter::Info,
        "warn" | "warning" | "40" => LevelFilter::Warn,
        "error" | "50" | "60" => LevelFilter::Error,
        "silent" | "off" => LevelFilter::Off,
        _ => LevelFilter::Info,
    }
}

struct BridgeLogger;

/// Read the JS logger level and publish it to `log::set_max_level` — the
/// `log` crate's macros check this static filter before calling `enabled()`,
/// so this is the single source of truth for level filtering.
fn sync_level_cache(js_logger: &JsLogger) {
    log::set_max_level(parse_level_filter(&js_logger.js_level()));
}

impl Log for BridgeLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // `log::set_max_level` already short-circuits below the desired level
        // before we are ever called, so every call that reaches us is enabled.
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        JS_LOGGER.with(|logger| {
            if let Some(ref js_logger) = *logger.borrow() {
                let obj = Object::new();

                let target = record.target();
                if !target.is_empty() {
                    let _ =
                        js_sys::Reflect::set(&obj, &JsValue::from_str("target"), &target.into());
                }

                let msg = record.args().to_string();

                match record.level() {
                    Level::Trace => js_logger.js_trace(&obj, Some(&msg)),
                    Level::Debug => js_logger.js_debug(&obj, Some(&msg)),
                    Level::Info => js_logger.js_info(&obj, Some(&msg)),
                    Level::Warn => js_logger.js_warn(&obj, Some(&msg)),
                    Level::Error => js_logger.js_error(&obj, Some(&msg)),
                }
            }
        });
    }

    fn flush(&self) {}
}

static BRIDGE_LOGGER: BridgeLogger = BridgeLogger;

#[wasm_bindgen(js_name = setLogger)]
pub fn set_logger(logger: JsLogger) -> Result<(), JsValue> {
    sync_level_cache(&logger);

    JS_LOGGER.with(|cell| {
        *cell.borrow_mut() = Some(logger);
    });

    if !LOGGER_INITIALIZED.swap(true, Ordering::SeqCst) {
        log::set_logger(&BRIDGE_LOGGER)
            .map_err(|e| JsValue::from_str(&format!("Failed to set logger: {}", e)))?;
    }

    Ok(())
}

#[wasm_bindgen(js_name = updateLogger)]
pub fn update_logger(logger: JsLogger) {
    sync_level_cache(&logger);

    JS_LOGGER.with(|cell| {
        *cell.borrow_mut() = Some(logger);
    });
}

#[wasm_bindgen(js_name = hasLogger)]
pub fn has_logger() -> bool {
    JS_LOGGER.with(|logger| logger.borrow().is_some())
}

/// Parse a pino-style level string to `log::Level`.
/// "off"/"silent" are upgraded to `Error` so the message is never silently dropped
/// by the single shared parser — callers that want true suppression should check
/// `log::max_level()` before invoking.
fn parse_level(level_str: &str) -> Level {
    parse_level_filter(level_str)
        .to_level()
        .unwrap_or(Level::Error)
}

#[wasm_bindgen(js_name = logMessage)]
pub fn log_message(level: &str, message: &str) {
    log::log!(parse_level(level), "{}", message);
}
