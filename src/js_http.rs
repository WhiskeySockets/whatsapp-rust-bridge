//! JS fetch()-based HTTP client adapter.
//!
//! Uses raw `js_sys::Function` to avoid wasm-bindgen reentrancy issues.

use async_trait::async_trait;
use js_sys::{Object, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use wacore::net::{HttpClient, HttpRequest, HttpResponse};

#[wasm_bindgen(typescript_custom_section)]
const TS_HTTP: &str = r#"
/**
 * JS HTTP client callbacks. Implement using fetch() or any HTTP library.
 */
export interface JsHttpClientConfig {
    execute(url: string, method: string, headers: Record<string, string>, body: Uint8Array | null): Promise<{ statusCode: number; body: Uint8Array }>;
}
"#;

/// Stores the execute function as a raw JS function.
pub struct JsHttpClientAdapter {
    execute_fn: js_sys::Function,
    _js_obj: JsValue,
}

crate::wasm_send_sync!(JsHttpClientAdapter);

impl JsHttpClientAdapter {
    pub fn from_js(obj: JsValue) -> Result<Self, JsValue> {
        let execute_fn = js_sys::Reflect::get(&obj, &"execute".into())?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| JsValue::from_str("httpClient.execute must be a function"))?;

        Ok(Self {
            execute_fn,
            _js_obj: obj,
        })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl HttpClient for JsHttpClientAdapter {
    async fn execute(&self, request: HttpRequest) -> anyhow::Result<HttpResponse> {
        let headers_obj = Object::new();
        for (key, value) in &request.headers {
            js_sys::Reflect::set(&headers_obj, &key.into(), &value.into())
                .map_err(|e| anyhow::anyhow!("set header: {e:?}"))?;
        }

        let body_js = match &request.body {
            Some(b) => Uint8Array::from(b.as_slice()).into(),
            None => JsValue::NULL,
        };

        let result = self
            .execute_fn
            .call4(
                &JsValue::NULL,
                &request.url.into(),
                &request.method.into(),
                &headers_obj.into(),
                &body_js,
            )
            .map_err(|e| anyhow::anyhow!("http execute: {e:?}"))?;

        let resolved = if result.is_instance_of::<js_sys::Promise>() {
            JsFuture::from(js_sys::Promise::unchecked_from_js(result))
                .await
                .map_err(|e| anyhow::anyhow!("http await: {e:?}"))?
        } else {
            result
        };

        let status_code = js_sys::Reflect::get(&resolved, &"statusCode".into())
            .map_err(|e| anyhow::anyhow!("get statusCode: {e:?}"))?
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("statusCode not a number"))?
            as u16;

        let body_val = js_sys::Reflect::get(&resolved, &"body".into())
            .map_err(|e| anyhow::anyhow!("get body: {e:?}"))?;

        let body = if body_val.is_instance_of::<Uint8Array>() {
            Uint8Array::unchecked_from_js(body_val).to_vec()
        } else {
            Vec::new()
        };

        Ok(HttpResponse { status_code, body })
    }
}
