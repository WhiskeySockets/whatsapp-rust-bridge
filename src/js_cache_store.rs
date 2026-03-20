//! Adapter that implements the Rust `CacheStore` trait by delegating to JS callbacks.
//!
//! This allows users to provide custom cache backends (Redis, Memcached, etc.)
//! from JavaScript while the Rust client uses them transparently.

use async_trait::async_trait;
use js_sys::{Function, Promise, Uint8Array};
use std::time::Duration;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use wacore::store::cache::CacheStore;

/// Wraps JS `JsCacheStore` callbacks to implement Rust's `CacheStore` trait.
pub struct JsCacheStoreAdapter {
    get_fn: Function,
    set_fn: Function,
    delete_fn: Function,
    clear_fn: Function,
}

crate::wasm_send_sync!(JsCacheStoreAdapter);

impl JsCacheStoreAdapter {
    /// Create from a JS object that has `get`, `set`, `delete`, `clear` methods.
    pub fn from_js(obj: &JsValue) -> Result<Self, JsValue> {
        let get_fn = js_sys::Reflect::get(obj, &"get".into())?
            .dyn_into::<Function>()
            .map_err(|_| JsValue::from_str("cacheStore.get must be a function"))?;
        let set_fn = js_sys::Reflect::get(obj, &"set".into())?
            .dyn_into::<Function>()
            .map_err(|_| JsValue::from_str("cacheStore.set must be a function"))?;
        let delete_fn = js_sys::Reflect::get(obj, &"delete".into())?
            .dyn_into::<Function>()
            .map_err(|_| JsValue::from_str("cacheStore.delete must be a function"))?;
        let clear_fn = js_sys::Reflect::get(obj, &"clear".into())?
            .dyn_into::<Function>()
            .map_err(|_| JsValue::from_str("cacheStore.clear must be a function"))?;

        Ok(Self {
            get_fn,
            set_fn,
            delete_fn,
            clear_fn,
        })
    }

    /// Check if a JS value looks like a JsCacheStore (has a `get` function).
    pub fn is_cache_store(val: &JsValue) -> bool {
        js_sys::Reflect::get(val, &"get".into())
            .map(|v| v.is_function())
            .unwrap_or(false)
    }
}

async fn resolve_promise(val: JsValue) -> Result<JsValue, anyhow::Error> {
    if val.is_instance_of::<Promise>() {
        let promise = Promise::unchecked_from_js(val);
        JsFuture::from(promise)
            .await
            .map_err(|e| anyhow::anyhow!("JS cache store error: {e:?}"))
    } else {
        Ok(val)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl CacheStore for JsCacheStoreAdapter {
    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let result = self
            .get_fn
            .call2(
                &JsValue::NULL,
                &JsValue::from_str(namespace),
                &JsValue::from_str(key),
            )
            .map_err(|e| anyhow::anyhow!("cache get error: {e:?}"))?;

        let resolved = resolve_promise(result).await?;

        if resolved.is_null() || resolved.is_undefined() {
            return Ok(None);
        }

        if resolved.is_instance_of::<Uint8Array>() {
            let arr = Uint8Array::unchecked_from_js(resolved);
            return Ok(Some(arr.to_vec()));
        }

        Ok(None)
    }

    async fn set(
        &self,
        namespace: &str,
        key: &str,
        value: &[u8],
        ttl: Option<Duration>,
    ) -> anyhow::Result<()> {
        let uint8 = Uint8Array::from(value);
        let ttl_js = match ttl {
            Some(d) => JsValue::from_f64(d.as_secs() as f64),
            None => JsValue::undefined(),
        };

        let result = self
            .set_fn
            .bind2(
                &JsValue::NULL,
                &JsValue::from_str(namespace),
                &JsValue::from_str(key),
            )
            .call2(&JsValue::NULL, &uint8.into(), &ttl_js)
            .map_err(|e| anyhow::anyhow!("cache set error: {e:?}"))?;

        resolve_promise(result).await?;
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
        let result = self
            .delete_fn
            .call2(
                &JsValue::NULL,
                &JsValue::from_str(namespace),
                &JsValue::from_str(key),
            )
            .map_err(|e| anyhow::anyhow!("cache delete error: {e:?}"))?;

        resolve_promise(result).await?;
        Ok(())
    }

    async fn clear(&self, namespace: &str) -> anyhow::Result<()> {
        let result = self
            .clear_fn
            .call1(&JsValue::NULL, &JsValue::from_str(namespace))
            .map_err(|e| anyhow::anyhow!("cache clear error: {e:?}"))?;

        resolve_promise(result).await?;
        Ok(())
    }
}
