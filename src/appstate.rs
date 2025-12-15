use js_sys::Uint8Array;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use wacore_appstate::{
    ExpandedAppStateKeys as RustExpandedAppStateKeys, LTHash, WAPATCH_INTEGRITY,
    expand_app_state_keys,
};

#[wasm_bindgen]
pub struct LTHashAntiTampering {
    inner: &'static LTHash,
}

#[wasm_bindgen]
impl LTHashAntiTampering {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: &WAPATCH_INTEGRITY,
        }
    }

    #[wasm_bindgen(js_name = subtractThenAdd)]
    pub fn subtract_then_add(
        &self,
        base: &[u8],
        subtract: Vec<Uint8Array>,
        add: Vec<Uint8Array>,
    ) -> Result<Uint8Array, JsValue> {
        if base.len() != 128 {
            return Err(JsValue::from_str(&format!(
                "Base hash must be 128 bytes, got {}",
                base.len()
            )));
        }

        let subtract_vecs: Vec<Vec<u8>> = subtract.iter().map(|arr| arr.to_vec()).collect();
        let add_vecs: Vec<Vec<u8>> = add.iter().map(|arr| arr.to_vec()).collect();

        let result = self
            .inner
            .subtract_then_add(base, &subtract_vecs, &add_vecs);

        let output = Uint8Array::new_with_length(result.len() as u32);
        output.copy_from(&result);
        Ok(output)
    }
}

impl Default for LTHashAntiTampering {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct ExpandedAppStateKeys {
    inner: RustExpandedAppStateKeys,
}

#[wasm_bindgen]
impl ExpandedAppStateKeys {
    #[wasm_bindgen(getter, js_name = indexKey)]
    pub fn index_key(&self) -> Uint8Array {
        let arr = Uint8Array::new_with_length(32);
        arr.copy_from(&self.inner.index);
        arr
    }

    #[wasm_bindgen(getter, js_name = valueEncryptionKey)]
    pub fn value_encryption_key(&self) -> Uint8Array {
        let arr = Uint8Array::new_with_length(32);
        arr.copy_from(&self.inner.value_encryption);
        arr
    }

    #[wasm_bindgen(getter, js_name = valueMacKey)]
    pub fn value_mac_key(&self) -> Uint8Array {
        let arr = Uint8Array::new_with_length(32);
        arr.copy_from(&self.inner.value_mac);
        arr
    }

    #[wasm_bindgen(getter, js_name = snapshotMacKey)]
    pub fn snapshot_mac_key(&self) -> Uint8Array {
        let arr = Uint8Array::new_with_length(32);
        arr.copy_from(&self.inner.snapshot_mac);
        arr
    }

    #[wasm_bindgen(getter, js_name = patchMacKey)]
    pub fn patch_mac_key(&self) -> Uint8Array {
        let arr = Uint8Array::new_with_length(32);
        arr.copy_from(&self.inner.patch_mac);
        arr
    }
}

#[wasm_bindgen(js_name = expandAppStateKeys)]
pub fn expand_app_state_keys_wasm(key_data: &[u8]) -> ExpandedAppStateKeys {
    let inner = expand_app_state_keys(key_data);
    ExpandedAppStateKeys { inner }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[wasm_bindgen]
pub struct LTHashState {
    version: u64,
    #[serde(with = "serde_bytes")]
    hash: Vec<u8>,
    #[serde(skip)]
    index_value_map: std::collections::HashMap<String, Vec<u8>>,
}

#[wasm_bindgen]
impl LTHashState {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            version: 0,
            hash: vec![0u8; 128],
            index_value_map: std::collections::HashMap::new(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn version(&self) -> u64 {
        self.version
    }

    #[wasm_bindgen(setter)]
    pub fn set_version(&mut self, version: u64) {
        self.version = version;
    }

    #[wasm_bindgen(getter)]
    pub fn hash(&self) -> Uint8Array {
        let arr = Uint8Array::new_with_length(self.hash.len() as u32);
        arr.copy_from(&self.hash);
        arr
    }

    #[wasm_bindgen(setter)]
    pub fn set_hash(&mut self, hash: &[u8]) {
        self.hash = hash.to_vec();
    }

    #[wasm_bindgen(js_name = getValueMac)]
    pub fn get_value_mac(&self, index_mac_base64: &str) -> Option<Uint8Array> {
        self.index_value_map.get(index_mac_base64).map(|v| {
            let arr = Uint8Array::new_with_length(v.len() as u32);
            arr.copy_from(v);
            arr
        })
    }

    #[wasm_bindgen(js_name = setValueMac)]
    pub fn set_value_mac(&mut self, index_mac_base64: &str, value_mac: &[u8]) {
        self.index_value_map
            .insert(index_mac_base64.to_string(), value_mac.to_vec());
    }

    #[wasm_bindgen(js_name = deleteValueMac)]
    pub fn delete_value_mac(&mut self, index_mac_base64: &str) -> bool {
        self.index_value_map.remove(index_mac_base64).is_some()
    }

    #[wasm_bindgen(js_name = hasValueMac)]
    pub fn has_value_mac(&self, index_mac_base64: &str) -> bool {
        self.index_value_map.contains_key(index_mac_base64)
    }

    #[wasm_bindgen(js_name = clone)]
    pub fn clone_state(&self) -> LTHashState {
        self.clone()
    }
}

impl Default for LTHashState {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen(js_name = generateContentMac)]
pub fn generate_content_mac(
    operation: u8,
    data: &[u8],
    key_id: &[u8],
    key: &[u8],
) -> Result<Uint8Array, JsValue> {
    use wacore_libsignal::crypto::CryptographicMac;

    if key.len() != 32 {
        return Err(JsValue::from_str(&format!(
            "Value MAC key must be 32 bytes, got {}",
            key.len()
        )));
    }

    let op_byte = [operation];
    let key_data_length = ((key_id.len() + 1) as u64).to_be_bytes();

    let mut mac = CryptographicMac::new("HmacSha512", key)
        .map_err(|e| JsValue::from_str(&format!("Failed to create MAC: {}", e)))?;
    mac.update(&op_byte);
    mac.update(key_id);
    mac.update(data);
    mac.update(&key_data_length);
    let mac_full = mac.finalize();

    let result = Uint8Array::new_with_length(32);
    result.copy_from(&mac_full[..32]);
    Ok(result)
}

#[wasm_bindgen(js_name = generateSnapshotMac)]
pub fn generate_snapshot_mac(
    lt_hash: &[u8],
    version: u64,
    name: &str,
    key: &[u8],
) -> Result<Uint8Array, JsValue> {
    use wacore_libsignal::crypto::CryptographicMac;

    if lt_hash.len() != 128 {
        return Err(JsValue::from_str(&format!(
            "LT-Hash must be 128 bytes, got {}",
            lt_hash.len()
        )));
    }

    if key.len() != 32 {
        return Err(JsValue::from_str(&format!(
            "Snapshot MAC key must be 32 bytes, got {}",
            key.len()
        )));
    }

    let version_be = version.to_be_bytes();

    let mut mac = CryptographicMac::new("HmacSha256", key)
        .map_err(|e| JsValue::from_str(&format!("Failed to create MAC: {}", e)))?;
    mac.update(lt_hash);
    mac.update(&version_be);
    mac.update(name.as_bytes());
    let mac_result = mac.finalize();

    let result = Uint8Array::new_with_length(mac_result.len() as u32);
    result.copy_from(&mac_result);
    Ok(result)
}

#[wasm_bindgen(js_name = generatePatchMac)]
pub fn generate_patch_mac(
    snapshot_mac: &[u8],
    value_macs: Vec<Uint8Array>,
    version: u64,
    name: &str,
    key: &[u8],
) -> Result<Uint8Array, JsValue> {
    use wacore_libsignal::crypto::CryptographicMac;

    if key.len() != 32 {
        return Err(JsValue::from_str(&format!(
            "Patch MAC key must be 32 bytes, got {}",
            key.len()
        )));
    }

    let version_be = version.to_be_bytes();

    let mut mac = CryptographicMac::new("HmacSha256", key)
        .map_err(|e| JsValue::from_str(&format!("Failed to create MAC: {}", e)))?;
    mac.update(snapshot_mac);
    for value_mac in &value_macs {
        mac.update(&value_mac.to_vec());
    }
    mac.update(&version_be);
    mac.update(name.as_bytes());
    let mac_result = mac.finalize();

    let result = Uint8Array::new_with_length(mac_result.len() as u32);
    result.copy_from(&mac_result);
    Ok(result)
}

#[wasm_bindgen(js_name = generateIndexMac)]
pub fn generate_index_mac(index_bytes: &[u8], key: &[u8]) -> Result<Uint8Array, JsValue> {
    use wacore_libsignal::crypto::CryptographicMac;

    if key.len() != 32 {
        return Err(JsValue::from_str(&format!(
            "Index key must be 32 bytes, got {}",
            key.len()
        )));
    }

    let mut mac = CryptographicMac::new("HmacSha256", key)
        .map_err(|e| JsValue::from_str(&format!("Failed to create MAC: {}", e)))?;
    mac.update(index_bytes);
    let mac_result = mac.finalize();

    let result = Uint8Array::new_with_length(mac_result.len() as u32);
    result.copy_from(&mac_result);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lt_hash_new() {
        let lt_hash = LTHashAntiTampering::new();
        assert_eq!(lt_hash.inner.hkdf_size, 128);
    }

    #[test]
    fn test_expand_app_state_keys() {
        let key = [7u8; 32];
        let expanded = expand_app_state_keys_wasm(&key);

        assert_eq!(expanded.index_key().length(), 32);
        assert_eq!(expanded.value_encryption_key().length(), 32);
        assert_eq!(expanded.value_mac_key().length(), 32);
        assert_eq!(expanded.snapshot_mac_key().length(), 32);
        assert_eq!(expanded.patch_mac_key().length(), 32);
    }

    #[test]
    fn test_lt_hash_state_new() {
        let state = LTHashState::new();
        assert_eq!(state.version(), 0);
        assert_eq!(state.hash.len(), 128);
    }
}
