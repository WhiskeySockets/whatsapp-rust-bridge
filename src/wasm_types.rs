use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const T_NODE: &'static str = r#"
/**
 * Represents the Wasm handle to a decoded binary node.
 * This object wraps a pointer into Wasm memory and exposes
 * lightweight accessor methods to read data on demand.
 */
export class WasmNode {
    readonly tag: string;
    readonly children: INode[];
    readonly content?: Uint8Array;

    getAttribute(key: string): string | undefined;
    getAttributeAsJid(key: string): string | undefined;

    getAttributes(): { [key: string]: string };
}

/**
 * Represents a node structure for ENCODING.
 * This is the plain JavaScript object representation passed to `encodeNode`.
 */
export interface INode {
    tag: string;
    attrs: { [key: string]: string };
    content?: INode[] | string | Uint8Array;
}
"#;

#[wasm_bindgen(typescript_custom_section)]
const T_SIGNAL: &'static str = r#"
/**
* Represents the data structure for a pre-key used in the Signal protocol.
*/
export type PreKey = {
    keyId: number;
    publicKey: Uint8Array;
    privateKey: Uint8Array;
};

/**
* Represents the data structure for a signed pre-key, including its signature.
*/
export type SignedPreKey = PreKey & {
    signature: Uint8Array;
};

/**
 * The storage interface required by the WASM libsignal implementation.
 * This must be implemented on the TypeScript side and passed to the WASM functions.
 * It mirrors the storage mechanism Baileys uses.
 */
export interface SignalStore {
    // --- Identity Methods ---
    getIdentityKeyPair(): Promise<PreKey | undefined>;
    getLocalRegistrationId(): Promise<number | undefined>;
    saveIdentity(address: string, identityKey: Uint8Array): Promise<boolean>;
    isTrustedIdentity(address: string, identityKey: Uint8Array, direction: 'sending' | 'receiving'): Promise<boolean>;
    getIdentity(address: string): Promise<Uint8Array | undefined>;

    // --- PreKey Methods ---
    loadPreKey(keyId: number): Promise<PreKey | undefined>;
    storePreKey(keyId: number, key: PreKey): Promise<void>;
    removePreKey(keyId: number): Promise<void>;
    
    // --- Signed PreKey Methods ---
    loadSignedPreKey(keyId: number): Promise<SignedPreKey | undefined>;
    storeSignedPreKey(keyId: number, key: SignedPreKey): Promise<void>;

    // --- Session Methods ---
    loadSession(address: string): Promise<Uint8Array | undefined>;
    storeSession(address: string, session: Uint8Array): Promise<void>;
    
    // --- Sender Key Methods ---
    loadSenderKey(name: string): Promise<Uint8Array | undefined>;
    storeSenderKey(name: string, key: Uint8Array): Promise<void>;
}

/**
 * Represents the output of an encryption operation.
 */
export type EncryptResult = {
    type: 'pkmsg' | 'msg';
    ciphertext: Uint8Array;
    // registrationId and identityKey are included for pkmsg types
    registrationId?: number;
    identityKey?: Uint8Array;
};

/**
 * Represents a pre-key bundle retrieved for establishing a new session.
 */
export interface PreKeyBundle {
    registrationId: number,
    deviceId: number,
    preKeyId?: number,
    preKeyPublic?: Uint8Array,
    signedPreKeyId: number,
    signedPreKeyPublic: Uint8Array,
    signedPreKeySignature: Uint8Array,
    identityKey: Uint8Array,
}
"#;
