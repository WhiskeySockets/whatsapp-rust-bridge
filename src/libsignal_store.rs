use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wacore_libsignal::protocol::SenderKeyRecord;
use wacore_libsignal::protocol::{
    error::Result, IdentityKeyStore, PreKeyStore, ProtocolStore, SessionStore, SignedPreKeyStore,
};
use wacore_libsignal::protocol::{
    Direction, IdentityChange, IdentityKey, IdentityKeyPair, PreKeyId, PreKeyRecord,
    ProtocolAddress, SessionRecord, SignedPreKeyId, SignedPreKeyRecord,
};
use wacore_libsignal::store::sender_key_name::SenderKeyName;

/// A fully functional in-memory store for Signal protocol state.
/// Implements all required traits for encryption/decryption operations.
#[derive(Clone, Default)]
pub struct InMemorySignalStore {
    identities: Arc<Mutex<HashMap<String, IdentityKey>>>,
    pre_keys: Arc<Mutex<HashMap<PreKeyId, PreKeyRecord>>>,
    signed_pre_keys: Arc<Mutex<HashMap<SignedPreKeyId, SignedPreKeyRecord>>>,
    sessions: Arc<Mutex<HashMap<String, SessionRecord>>>,
    identity_key_pair: Arc<Mutex<Option<IdentityKeyPair>>>,
    registration_id: Arc<Mutex<Option<u32>>>,
    // Track removed pre_key to inform the JS side
    removed_pre_key: Arc<Mutex<Option<PreKeyId>>>,
}

impl InMemorySignalStore {
    pub fn new() -> Self {
        Self::default()
    }

    // --- Methods to populate the store from JS ---
    pub fn set_identity_key_pair(&self, identity_key_pair: IdentityKeyPair) {
        *self.identity_key_pair.lock().unwrap() = Some(identity_key_pair);
    }

    pub fn set_registration_id(&self, registration_id: u32) {
        *self.registration_id.lock().unwrap() = Some(registration_id);
    }

    pub fn set_session(&self, address: &str, record: SessionRecord) {
        self.sessions
            .lock()
            .unwrap()
            .insert(address.to_string(), record);
    }

    pub fn add_pre_key(&self, id: PreKeyId, record: PreKeyRecord) {
        self.pre_keys.lock().unwrap().insert(id, record);
    }

    pub fn add_signed_pre_key(&self, id: SignedPreKeyId, record: SignedPreKeyRecord) {
        self.signed_pre_keys.lock().unwrap().insert(id, record);
    }

    // --- Methods to retrieve results for JS ---
    pub fn get_session(&self, address: &str) -> Option<SessionRecord> {
        self.sessions.lock().unwrap().get(address).cloned()
    }

    pub fn get_removed_pre_key_id(&self) -> Option<PreKeyId> {
        *self.removed_pre_key.lock().unwrap()
    }
}

// --- Trait Implementations for wacore-libsignal ---
#[async_trait]
impl IdentityKeyStore for InMemorySignalStore {
    async fn get_identity_key_pair(&self) -> Result<IdentityKeyPair> {
        (*self.identity_key_pair.lock().unwrap()).ok_or_else(|| {
            wacore_libsignal::protocol::SignalProtocolError::InvalidState(
                "InMemorySignalStore",
                "IdentityKeyPair not set".into(),
            )
        })
    }

    async fn get_local_registration_id(&self) -> Result<u32> {
        self.registration_id.lock().unwrap().ok_or_else(|| {
            wacore_libsignal::protocol::SignalProtocolError::InvalidState(
                "InMemorySignalStore",
                "RegistrationId not set".into(),
            )
        })
    }

    async fn save_identity(
        &mut self,
        address: &ProtocolAddress,
        identity: &IdentityKey,
    ) -> Result<IdentityChange> {
        let changed = self
            .identities
            .lock()
            .unwrap()
            .insert(address.name().to_string(), *identity)
            .is_some();
        Ok(IdentityChange::from_changed(changed))
    }

    async fn is_trusted_identity(
        &self,
        _: &ProtocolAddress,
        _: &IdentityKey,
        _: Direction,
    ) -> Result<bool> {
        Ok(true) // Baileys compatibility: always trust
    }

    async fn get_identity(&self, address: &ProtocolAddress) -> Result<Option<IdentityKey>> {
        Ok(self.identities.lock().unwrap().get(address.name()).cloned())
    }
}

#[async_trait]
impl SessionStore for InMemorySignalStore {
    async fn load_session(&self, address: &ProtocolAddress) -> Result<Option<SessionRecord>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .get(&address.to_string())
            .cloned())
    }

    async fn store_session(
        &mut self,
        address: &ProtocolAddress,
        record: &SessionRecord,
    ) -> Result<()> {
        self.sessions
            .lock()
            .unwrap()
            .insert(address.to_string(), record.clone());
        Ok(())
    }
}

#[async_trait]
impl PreKeyStore for InMemorySignalStore {
    async fn get_pre_key(&self, id: PreKeyId) -> Result<PreKeyRecord> {
        self.pre_keys
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                wacore_libsignal::protocol::SignalProtocolError::InvalidState(
                    "InMemorySignalStore",
                    format!("PreKey {} not found", id),
                )
            })
    }

    async fn save_pre_key(&mut self, id: PreKeyId, record: &PreKeyRecord) -> Result<()> {
        self.pre_keys.lock().unwrap().insert(id, record.clone());
        Ok(())
    }

    async fn remove_pre_key(&mut self, id: PreKeyId) -> Result<()> {
        self.pre_keys.lock().unwrap().remove(&id);
        *self.removed_pre_key.lock().unwrap() = Some(id);
        Ok(())
    }
}

#[async_trait]
impl SignedPreKeyStore for InMemorySignalStore {
    async fn get_signed_pre_key(&self, id: SignedPreKeyId) -> Result<SignedPreKeyRecord> {
        self.signed_pre_keys
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                wacore_libsignal::protocol::SignalProtocolError::InvalidState(
                    "InMemorySignalStore",
                    format!("SignedPreKey {} not found", id),
                )
            })
    }

    async fn save_signed_pre_key(
        &mut self,
        id: SignedPreKeyId,
        record: &SignedPreKeyRecord,
    ) -> Result<()> {
        self.signed_pre_keys
            .lock()
            .unwrap()
            .insert(id, record.clone());
        Ok(())
    }
}

// Dummy implementation for group messages
#[async_trait]
impl wacore_libsignal::protocol::SenderKeyStore for InMemorySignalStore {
    async fn store_sender_key(&mut self, _: &SenderKeyName, _: &SenderKeyRecord) -> Result<()> {
        unimplemented!("SenderKeyStore not implemented in InMemorySignalStore")
    }

    async fn load_sender_key(&mut self, _: &SenderKeyName) -> Result<Option<SenderKeyRecord>> {
        unimplemented!("SenderKeyStore not implemented in InMemorySignalStore")
    }
}

impl ProtocolStore for InMemorySignalStore {}
