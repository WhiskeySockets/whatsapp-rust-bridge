//! Cross-boundary error type for the WASM bridge.
//!
//! `BridgeError` is the single shape that all errors take when they cross the
//! WASM→JS boundary. It is built by walking the `std::error::Error::source()`
//! chain of whatever the core returned and downcasting to the leaf types whose
//! structured fields (e.g. `IqError::ServerError { code, text }`) we want to
//! preserve in JS.
//!
//! On the JS side the result is a `js_sys::Error` named `WhatsAppError` with
//! the discriminant on `.kind` and per-variant fields set as own properties —
//! consumers narrow via `e.kind === 'server'` and read `e.serverCode`, etc.
//! See `From<BridgeError> for JsValue` and `to_js_error` for the conversion.

use core::error::Error;

use serde::Serialize;
use thiserror::Error;
use tsify_next::Tsify;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue};

use whatsapp_rust::pair_code::{PairCodeError, PairError};
use whatsapp_rust::request::IqError;

/// Public error shape that crosses the WASM→JS boundary.
///
/// Variants are intentionally flat — no `#[from]` on enum variants, no
/// `#[serde(flatten)]`. Translation from the core's typed errors happens in
/// `From` impls below by walking the source chain. This keeps the JS object
/// shape predictable and the codegen / `Tsify` output simple.
// `Tsify` is used here only for `.d.ts` generation — we deliberately do NOT
// add `#[tsify(into_wasm_abi)]` because that auto-generates a
// `From<BridgeError> for JsValue` that would produce a plain object instead
// of a real `js_sys::Error`. The custom `From` impl below builds an `Error`
// (so `instanceof Error` works in JS) and copies the serde-payload's fields
// onto it as own properties.
#[derive(Debug, Error, Serialize, Tsify)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BridgeError {
    /// The WhatsApp server returned a structured `<error code="..." text="..."/>`
    /// stanza in response to an IQ. `code`/`text` are the server-supplied
    /// fields; consumers usually branch on `code` (400/401/404/406/...).
    #[error("server error {server_code}: {server_text}")]
    Server {
        #[serde(rename = "serverCode")]
        server_code: u16,
        #[serde(rename = "serverText")]
        server_text: String,
    },

    /// The IQ request did not get a response within the timeout window.
    #[error("request timed out")]
    Timeout,

    /// No active socket / not authenticated. Caller must reconnect / log in
    /// before retrying.
    #[error("not connected")]
    NotConnected,

    /// Server pushed a `<stream:error>` / disconnect node mid-flight. The raw
    /// reason text (when available) is on `reason`.
    #[error("disconnected by server: {reason}")]
    Disconnected { reason: String },

    /// Caller passed something the API can statically reject — empty phone
    /// number, malformed JID, custom pair code outside the Crockford alphabet,
    /// etc. `field` names which input was rejected; `reason` describes why.
    #[error("invalid argument {field}: {reason}")]
    InvalidArgument { field: String, reason: String },

    /// The remote peer sent something that violates the protocol — e.g. an
    /// invalid public key during pairing, an unexpected stanza shape, missing
    /// required attributes. Different from `Server` in that the *server*
    /// (or peer) sent something we couldn't parse, vs. returning a typed
    /// error stanza.
    #[error("protocol violation: {reason}")]
    ProtocolViolation { reason: String },

    /// A cryptographic operation failed — key parse, DH agreement, AEAD
    /// encrypt/decrypt, HKDF expand. `operation` describes which step.
    #[error("crypto operation failed: {operation}")]
    Crypto { operation: String },

    /// Persistence layer failure — JS-backed callbacks errored, serde failed,
    /// SQLite was busy/locked, etc.
    #[error("storage operation failed: {operation}")]
    Storage { operation: String },

    /// Catch-all for cases not yet mapped. The full `Display` chain is in
    /// `message` so JS-side debugging is still possible.
    #[error("internal: {message}")]
    Internal { message: String },
}

impl BridgeError {
    /// Walk a borrowed `&dyn Error` chain looking for known leaf types whose
    /// structured fields we expose on the JS surface. The walk has two
    /// priority tiers:
    ///
    /// 1. **High-priority leaves** (return immediately): typed errors whose
    ///    discriminant is the most informative thing the JS side can get —
    ///    `IqError::ServerError { code, text }`, `PairCodeError` validation
    ///    variants, JS-backed storage callback errors. If a parent like
    ///    `IqError::Socket(_)` is purely a wrapper, `iq_to_bridge` returns
    ///    `None` and the loop continues down `source()` to find the real
    ///    leaf below.
    ///
    /// 2. **Generic wrappers** (remembered as fallback): `StoreError` is
    ///    useful when nothing more specific is found below it, but a JS
    ///    callback or JSON-deserialize source nested inside it is even
    ///    more actionable. We capture the first wrapper hit and only
    ///    surface it if the deeper walk turns up nothing typed.
    ///
    /// Falls back to `Internal { message }` carrying the full chain
    /// `Display` if neither tier matched.
    pub fn from_error_chain(err: &(dyn Error + 'static)) -> Self {
        let mut storage_fallback: Option<BridgeError> = None;
        let mut cur: Option<&(dyn Error + 'static)> = Some(err);

        while let Some(c) = cur {
            // ── High-priority leaves: return on hit ──────────────────────
            if let Some(iq) = c.downcast_ref::<IqError>() {
                if let Some(b) = iq_to_bridge(iq) {
                    return b;
                }
                // wrapper variant — keep walking via .source()
            } else if let Some(iq) = c.downcast_ref::<wacore::request::IqError>() {
                if let Some(b) = wacore_iq_to_bridge(iq) {
                    return b;
                }
            } else if let Some(pc) = c.downcast_ref::<PairCodeError>() {
                return paircode_to_bridge(pc);
            } else if let Some(jc) = c.downcast_ref::<crate::js_backend::JsCallbackError>() {
                return BridgeError::Storage {
                    operation: jc.to_string(),
                };
            } else if let Some(js) = c.downcast_ref::<crate::js_backend::JsonStoreError>() {
                return BridgeError::Storage {
                    operation: js.to_string(),
                };
            }
            // ── Generic wrapper: remember first hit, keep walking ────────
            else if storage_fallback.is_none()
                && let Some(se) = c.downcast_ref::<wacore::store::error::StoreError>()
            {
                storage_fallback = Some(BridgeError::Storage {
                    operation: se.to_string(),
                });
            }

            cur = c.source();
        }

        if let Some(b) = storage_fallback {
            return b;
        }
        BridgeError::Internal {
            message: display_chain(err),
        }
    }
}

/// Match a high-level `IqError` against variants whose fields are themselves
/// the user-actionable signal. Wrapper variants (`Socket`, `EncryptSend`,
/// `ClientState`, `EncodeError`, `ParseError`) return `None` so the chain
/// walker keeps descending into `.source()` — there may be a typed leaf
/// below worth surfacing.
fn iq_to_bridge(e: &IqError) -> Option<BridgeError> {
    Some(match e {
        IqError::ServerError { code, text } => BridgeError::Server {
            server_code: *code,
            server_text: text.clone(),
        },
        IqError::Timeout => BridgeError::Timeout,
        IqError::NotConnected => BridgeError::NotConnected,
        IqError::Disconnected(node) => BridgeError::Disconnected {
            reason: format!("{node:?}"),
        },
        IqError::InternalChannelClosed => BridgeError::Internal {
            message: e.to_string(),
        },
        // Wrappers — let the walker drill into `.source()`.
        IqError::Socket(_)
        | IqError::EncryptSend(_)
        | IqError::ClientState(_)
        | IqError::EncodeError(_)
        | IqError::ParseError(_) => return None,
    })
}

fn wacore_iq_to_bridge(e: &wacore::request::IqError) -> Option<BridgeError> {
    Some(match e {
        wacore::request::IqError::ServerError { code, text } => BridgeError::Server {
            server_code: *code,
            server_text: text.clone(),
        },
        wacore::request::IqError::Timeout => BridgeError::Timeout,
        wacore::request::IqError::NotConnected => BridgeError::NotConnected,
        wacore::request::IqError::Disconnected(node) => BridgeError::Disconnected {
            reason: format!("{node:?}"),
        },
        wacore::request::IqError::InternalChannelClosed => BridgeError::Internal {
            message: e.to_string(),
        },
    })
}

fn paircode_to_bridge(e: &PairCodeError) -> BridgeError {
    use BridgeError::*;
    match e {
        PairCodeError::PhoneNumberRequired => InvalidArgument {
            field: "phoneNumber".into(),
            reason: "required".into(),
        },
        PairCodeError::PhoneNumberTooShort => InvalidArgument {
            field: "phoneNumber".into(),
            reason: "too short (min 7 digits)".into(),
        },
        PairCodeError::PhoneNumberNotInternational => InvalidArgument {
            field: "phoneNumber".into(),
            reason: "must not start with 0 (use international format)".into(),
        },
        PairCodeError::InvalidCustomCode => InvalidArgument {
            field: "customCode".into(),
            reason: "must be 8 chars from Crockford Base32 alphabet".into(),
        },
        PairCodeError::InvalidWrappedData { expected, got } => ProtocolViolation {
            reason: format!("wrapped data: expected {expected} bytes, got {got}"),
        },
        PairCodeError::InvalidPrimaryEphemeralKey(_) => Crypto {
            operation: "parse primary ephemeral key".into(),
        },
        PairCodeError::InvalidPrimaryIdentityKey(_) => Crypto {
            operation: "parse primary identity key".into(),
        },
        PairCodeError::EphemeralKeyAgreement(_) => Crypto {
            operation: "ephemeral DH agreement".into(),
        },
        PairCodeError::IdentityKeyAgreement(_) => Crypto {
            operation: "identity DH agreement".into(),
        },
        PairCodeError::AdvSecretKeyDerivation => Crypto {
            operation: "HKDF expand for adv_secret".into(),
        },
        PairCodeError::BundleKeyDerivation => Crypto {
            operation: "HKDF expand for bundle encryption key".into(),
        },
        PairCodeError::BundleAead(_) => Crypto {
            operation: "AES-GCM encrypt key bundle".into(),
        },
        PairCodeError::NotWaiting => ProtocolViolation {
            reason: "not in waiting state for pair-code notification".into(),
        },
        PairCodeError::MissingPairingRef => ProtocolViolation {
            reason: "server response missing pairing ref".into(),
        },
    }
}

/// Render the full `Display` chain of an error, separated by `: `. Mirrors
/// what `tracing::error!(error = ?e)` would log if we did it manually.
fn display_chain(err: &(dyn Error + 'static)) -> String {
    use core::fmt::Write;
    let mut out = String::new();
    let _ = write!(out, "{err}");
    let mut cur = err.source();
    while let Some(c) = cur {
        let _ = write!(out, ": {c}");
        cur = c.source();
    }
    out
}

// ---------------------------------------------------------------------------
// From impls — typed entry points that walk the source chain.
//
// We can't blanket `impl<E: Error + 'static> From<E> for BridgeError` because
// it conflicts with the blanket `From<T> for T`. So we list the entry points
// the wasm boundary actually returns. New error types added at call sites
// only need their own `From` here — the chain walking generalizes.

impl From<PairError> for BridgeError {
    fn from(e: PairError) -> Self {
        Self::from_error_chain(&e)
    }
}

impl From<IqError> for BridgeError {
    fn from(e: IqError) -> Self {
        Self::from_error_chain(&e)
    }
}

impl From<PairCodeError> for BridgeError {
    fn from(e: PairCodeError) -> Self {
        paircode_to_bridge(&e)
    }
}

/// Bulk of the core's high-level methods return `anyhow::Error`. Walk the
/// chain — `anyhow::Error` derefs to `dyn Error + Send + Sync + 'static`
/// so the same downcast machinery works.
impl From<anyhow::Error> for BridgeError {
    fn from(e: anyhow::Error) -> Self {
        Self::from_error_chain(e.as_ref())
    }
}

/// Lower-level wacore error wrappers we hit at the boundary occasionally
/// (e.g. `parse_jid`, IQ encoding, binary parsing). Mapped to specific
/// variants where the discriminant is useful; otherwise falls through to
/// `Internal` via the chain walk.
impl From<wacore_binary::jid::JidError> for BridgeError {
    fn from(e: wacore_binary::jid::JidError) -> Self {
        BridgeError::InvalidArgument {
            field: "jid".into(),
            reason: e.to_string(),
        }
    }
}

/// `serde_wasm_bindgen` errors: deserialization of JS arguments failed.
/// Always a caller bug — wrong shape supplied. `InvalidArgument` is the
/// right kind so consumers can surface "you passed bad input".
impl From<serde_wasm_bindgen::Error> for BridgeError {
    fn from(e: serde_wasm_bindgen::Error) -> Self {
        BridgeError::InvalidArgument {
            field: "input".into(),
            reason: e.to_string(),
        }
    }
}

/// Catch-all for raw `JsValue` errors crossing back from JS callbacks
/// (transport, http, store) — those don't have Rust-side type info, only
/// whatever `Display`/`Debug` representation JS gave us.
#[cfg(target_arch = "wasm32")]
impl From<JsValue> for BridgeError {
    fn from(e: JsValue) -> Self {
        let message = e.as_string().unwrap_or_else(|| format!("{e:?}"));
        BridgeError::Internal { message }
    }
}

/// MEX (GraphQL-over-IQ) errors. The most common variant carries an `IqError`
/// source — the chain walk picks it up — so this is just the entry impl.
impl From<whatsapp_rust::MexError> for BridgeError {
    fn from(e: whatsapp_rust::MexError) -> Self {
        Self::from_error_chain(&e)
    }
}

/// `ClientError` (top-level connection / send pipeline error) and
/// `PresenceError` (presence subscription / status update). Both wrap
/// typed sources (Socket, IqError, etc.) — the chain walk extracts them.
impl From<whatsapp_rust::client::ClientError> for BridgeError {
    fn from(e: whatsapp_rust::client::ClientError) -> Self {
        Self::from_error_chain(&e)
    }
}

impl From<whatsapp_rust::features::PresenceError> for BridgeError {
    fn from(e: whatsapp_rust::features::PresenceError) -> Self {
        Self::from_error_chain(&e)
    }
}

// Deliberate non-impl: `From<serde_json::Error>`. The right `kind` depends on
// where the JSON came from — caller-supplied input is `InvalidArgument`,
// remote/CDN responses are `ProtocolViolation`, internal serialization is
// `Internal`. A blanket impl would mis-classify CDN parse failures as caller
// bugs (see whatsapp-rust-bridge POC review). Use `BridgeError::invalid_arg`
// / `protocol_violation` / `internal` helpers (or build the variant inline)
// at each call site instead.

/// Slice → fixed-size-array conversion failures (e.g. media keys, hashes).
/// Indicates malformed/wrong-sized binary input — caller's responsibility.
impl From<core::array::TryFromSliceError> for BridgeError {
    fn from(e: core::array::TryFromSliceError) -> Self {
        BridgeError::InvalidArgument {
            field: "binary".into(),
            reason: e.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// JsValue conversion: build a real `js_sys::Error` (so `instanceof Error`
// works in JS) and copy the serde payload's fields onto it as own properties.
// JS-side, consumers see:
//   const e = ...;
//   e instanceof Error            // true
//   e.name === 'WhatsAppError'    // true
//   e.kind === 'server'           // narrows via discriminant
//   e.serverCode, e.serverText    // (camelCased by serde rename_all)

#[cfg(target_arch = "wasm32")]
impl From<BridgeError> for JsValue {
    fn from(e: BridgeError) -> JsValue {
        to_js_error(&e)
    }
}

/// Build an `Internal` variant from any `Display`able message. Convenience
/// wrapper used by call sites that previously returned `JsError::new(...)`
/// for ad-hoc messages with no typed source (e.g. argument validation that
/// the bridge does itself, before reaching the core).
pub fn internal<S: Into<String>>(message: S) -> BridgeError {
    BridgeError::Internal {
        message: message.into(),
    }
}

/// Build an `InvalidArgument` variant. Use at boundary sites where the
/// bridge rejects caller-supplied input *before* it reaches the core
/// (malformed JSON arguments, missing fields the JS layer should have
/// supplied).
pub fn invalid_arg<F: Into<String>, R: Into<String>>(field: F, reason: R) -> BridgeError {
    BridgeError::InvalidArgument {
        field: field.into(),
        reason: reason.into(),
    }
}

/// Build a `ProtocolViolation` variant. Use when a *remote* peer (server,
/// CDN, paired device) sent something we couldn't parse — distinct from
/// `Server` (which is a typed `<error>` stanza from WhatsApp) and from
/// `InvalidArgument` (which is the *caller's* fault, not the wire's).
pub fn protocol_violation<R: Into<String>>(reason: R) -> BridgeError {
    BridgeError::ProtocolViolation {
        reason: reason.into(),
    }
}

/// Construct a JS `Error` carrying the `BridgeError` payload. Takes `&` so
/// `Display` (via `e.to_string()`) and `serde::Serialize` (via
/// `serde_wasm_bindgen::to_value`) can both run without consuming.
#[cfg(target_arch = "wasm32")]
pub fn to_js_error(e: &BridgeError) -> JsValue {
    use js_sys::{Error as JsError, Object, Reflect};

    let err = JsError::new(&e.to_string());
    err.set_name("WhatsAppError");

    if let Ok(payload) = serde_wasm_bindgen::to_value(e)
        && let Some(obj) = payload.dyn_ref::<Object>()
    {
        let keys = Object::keys(obj);
        for key in keys.iter() {
            if let Ok(value) = Reflect::get(obj, &key) {
                let _ = Reflect::set(&err, &key, &value);
            }
        }
    }

    err.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iq_server_error_extracts_code_and_text() {
        let iq = IqError::ServerError {
            code: 400,
            text: "bad-request".into(),
        };
        let be: BridgeError = iq.into();
        match be {
            BridgeError::Server {
                server_code,
                server_text,
            } => {
                assert_eq!(server_code, 400);
                assert_eq!(server_text, "bad-request");
            }
            other => panic!("expected Server, got {other:?}"),
        }
    }

    #[test]
    fn pair_error_request_failed_walks_to_iq_server_error() {
        let iq = IqError::ServerError {
            code: 400,
            text: "bad-request".into(),
        };
        let pe: PairError = PairError::RequestFailed(iq);
        let be: BridgeError = pe.into();
        match be {
            BridgeError::Server {
                server_code,
                server_text,
            } => {
                assert_eq!(server_code, 400);
                assert_eq!(server_text, "bad-request");
            }
            other => panic!("expected Server via chain walk, got {other:?}"),
        }
    }

    #[test]
    fn pair_error_paircode_validation_walks_to_invalid_argument() {
        let pc = PairCodeError::PhoneNumberTooShort;
        let pe: PairError = PairError::PairCode(pc);
        let be: BridgeError = pe.into();
        match be {
            BridgeError::InvalidArgument { field, .. } => {
                assert_eq!(field, "phoneNumber");
            }
            other => panic!("expected InvalidArgument, got {other:?}"),
        }
    }

    #[test]
    fn iq_timeout_maps_to_timeout_kind() {
        let be: BridgeError = IqError::Timeout.into();
        assert!(matches!(be, BridgeError::Timeout));
    }

    #[test]
    fn iq_not_connected_maps_to_not_connected_kind() {
        let be: BridgeError = IqError::NotConnected.into();
        assert!(matches!(be, BridgeError::NotConnected));
    }

    #[test]
    fn unknown_iq_variant_lands_in_internal_with_full_chain() {
        // EncodeError carries an anyhow source; we want display_chain to walk it.
        let inner = anyhow::anyhow!("encoder blew up");
        let iq = IqError::EncodeError(inner);
        let be: BridgeError = iq.into();
        match be {
            BridgeError::Internal { message } => {
                assert!(message.contains("failed to encode"));
                assert!(message.contains("encoder blew up"));
            }
            other => panic!("expected Internal, got {other:?}"),
        }
    }
}
