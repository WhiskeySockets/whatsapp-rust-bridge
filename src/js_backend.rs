//! JS storage backend adapter.
//!
//! For the initial version, we use `InMemoryBackend` from wacore.
//! A full JS-callback-based Backend implementation can be added later
//! for persistence (IndexedDB, file system, etc.).

use std::sync::Arc;

use wacore::store::InMemoryBackend;
use wacore::store::traits::Backend;

/// Get a new InMemoryBackend instance (used internally).
pub(crate) fn new_in_memory_backend() -> Arc<dyn Backend> {
    Arc::new(InMemoryBackend::default())
}
