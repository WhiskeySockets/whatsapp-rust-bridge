#[cfg(target_family = "wasm")]
#[global_allocator]
static TALCK: talc::TalckWasm = unsafe { talc::TalckWasm::new_global() };

pub mod libsignal_api;
pub mod libsignal_store;
pub mod wasm_api;
pub mod wasm_types;
