// Type-safe wrapper over wasm-bindgen generated bindings.
// Re-exports the WASM API with precise types from generated.d.ts.
//
// This file is the public TypeScript API for whatsapp-rust-bridge.

import type {
  Event,
  MessageInfo,
  MessageSource,
  GroupInfo,
  GroupUpdate,
  Receipt,
  PairSuccess,
  PairError,
  LoggedOut,
  ConnectFailure,
  ConnectFailureReason,
  PresenceUpdate,
  ChatPresenceUpdate,
  DeviceListUpdate,
  OfflineSyncPreview,
  OfflineSyncCompleted,
  PictureUpdate,
  SelfPushNameUpdated,
  TemporaryBan,
  StreamError,
  UndecryptableMessage,
  DisappearingModeChanged,
  NewsletterLiveUpdate,
  BusinessStatusUpdate,
} from "./generated.js";

// Re-export all generated types
export * from "./generated.js";

// ---------------------------------------------------------------------------
// Transport & HTTP interfaces
// ---------------------------------------------------------------------------

/** Handle provided to connect() for pushing WebSocket events into the engine. */
export interface JsTransportHandle {
  /** Call when WebSocket connection opens. */
  onConnected(): void;
  /** Call when WebSocket receives binary data. */
  onData(data: Uint8Array): void;
  /** Call when WebSocket connection closes. */
  onDisconnected(): void;
}

/** WebSocket transport callbacks. */
export interface JsTransportCallbacks {
  /**
   * Create a WebSocket connection. Wire events to the handle:
   * - ws.onopen → handle.onConnected()
   * - ws.onmessage → handle.onData(data)
   * - ws.onclose → handle.onDisconnected()
   */
  connect(handle: JsTransportHandle): void | Promise<void>;
  /** Send raw bytes over the WebSocket. */
  send(data: Uint8Array): void | Promise<void>;
  /** Close the WebSocket. */
  disconnect(): void | Promise<void>;
}

/** HTTP client callbacks (implement using fetch). */
export interface JsHttpClientConfig {
  execute(
    url: string,
    method: string,
    headers: Record<string, string>,
    body: Uint8Array | null
  ): Promise<{ statusCode: number; body: Uint8Array }>;
}

// ---------------------------------------------------------------------------
// WhatsApp Client
// ---------------------------------------------------------------------------

/** Full WhatsApp client running in WASM. */
export interface WasmWhatsAppClient {
  /** Start the main loop (connect → handshake → message loop → reconnect). */
  run(): void;
  /** Single connection attempt (no auto-reconnect). */
  connect(): Promise<void>;
  /** Disconnect from WhatsApp servers. */
  disconnect(): Promise<void>;
  /** Check if WebSocket is currently connected. */
  isConnected(): boolean;
  /** Check if the client has completed pairing. */
  isLoggedIn(): boolean;

  /**
   * Send an E2E encrypted message.
   * @param jid Recipient JID (e.g. "5511999999999@s.whatsapp.net")
   * @param message Message content (matches wa.Message protobuf schema, snake_case keys)
   * @returns Message ID string
   */
  sendMessage(jid: string, message: WaMessage): Promise<string>;

  /** Fetch group metadata. */
  getGroupMetadata(jid: string): Promise<GroupMetadataResult>;
  /** Create a new group. */
  createGroup(subject: string, participants: string[]): Promise<{ gid: string }>;
  /** Check if a phone number is on WhatsApp. */
  isOnWhatsApp(phone: string): Promise<IsOnWhatsAppResult[]>;

  /** Set the user's display name. */
  setPushName(name: string): Promise<void>;
  /** Get the current display name. */
  getPushName(): Promise<string>;
  /** Get the own phone JID. */
  getJid(): Promise<string | undefined>;
  /** Get the own LID (linked identity). */
  getLid(): Promise<string | undefined>;

  /** Send presence ("available" or "unavailable"). */
  sendPresence(status: "available" | "unavailable"): Promise<void>;
  /** Send typing indicator. */
  sendChatState(jid: string, state: "composing" | "recording" | "paused"): Promise<void>;

  /** Free WASM resources. Call when done with the client. */
  free(): void;
}

/** Message content for sendMessage (snake_case to match prost serialization). */
export interface WaMessage {
  conversation?: string;
  extended_text_message?: {
    text?: string;
    context_info?: any;
  };
  image_message?: any;
  video_message?: any;
  audio_message?: any;
  document_message?: any;
  protocol_message?: any;
  [key: string]: any;
}

/** Result from getGroupMetadata. */
export interface GroupMetadataResult {
  id: string;
  subject: string;
  participants: Array<{
    jid: string;
    phoneNumber?: string;
    isAdmin: boolean;
  }>;
  addressingMode: string;
  creator?: string;
  creationTime?: number;
  subjectTime?: number;
  subjectOwner?: string;
  description?: string;
  descriptionId?: string;
  isLocked: boolean;
  isAnnouncement: boolean;
  ephemeralExpiration: number;
  membershipApproval: boolean;
  size?: number;
  isParentGroup: boolean;
  parentGroupJid?: string;
  isDefaultSubGroup: boolean;
  isGeneralChat: boolean;
}

/** Result from isOnWhatsApp. */
export interface IsOnWhatsAppResult {
  jid: string;
  isRegistered: boolean;
}

// ---------------------------------------------------------------------------
// Client creation
// ---------------------------------------------------------------------------

/** Initialize the WASM engine. Call once before creating clients. */
export declare function initWasmEngine(): void;

/**
 * Create a full WhatsApp client running in WASM.
 *
 * @param transport WebSocket transport callbacks
 * @param httpClient HTTP client callbacks (for media, version fetching)
 * @param onEvent Event callback — receives typed WhatsApp events
 */
export declare function createWhatsAppClient(
  transport: JsTransportCallbacks,
  httpClient: JsHttpClientConfig,
  onEvent?: (event: Event) => void
): Promise<WasmWhatsAppClient>;

// ---------------------------------------------------------------------------
// Proto encode/decode
// ---------------------------------------------------------------------------

/** Encode a protobuf message to binary. */
export declare function encodeProto(typeName: string, json: any): Uint8Array;
/** Decode binary protobuf to a JS object. */
export declare function decodeProto(typeName: string, data: Uint8Array): any;

// Type-specific encode/decode removed — use encodeProto/decodeProto with a type name instead.
// Example: encodeProto("Message", json) / decodeProto("Message", data)

// ---------------------------------------------------------------------------
// Low-level Signal protocol (for advanced use)
// ---------------------------------------------------------------------------

export { NoiseSession } from "../pkg/whatsapp_rust_bridge.js";
export { SessionCipher } from "../pkg/whatsapp_rust_bridge.js";
export { SessionBuilder } from "../pkg/whatsapp_rust_bridge.js";
export { GroupCipher } from "../pkg/whatsapp_rust_bridge.js";
export { GroupSessionBuilder } from "../pkg/whatsapp_rust_bridge.js";
export { ProtocolAddress } from "../pkg/whatsapp_rust_bridge.js";
export { SessionRecord } from "../pkg/whatsapp_rust_bridge.js";
export { SenderKeyName } from "../pkg/whatsapp_rust_bridge.js";
export { SenderKeyRecord } from "../pkg/whatsapp_rust_bridge.js";
export { SenderKeyDistributionMessage } from "../pkg/whatsapp_rust_bridge.js";

// Key generation
export {
  generateKeyPair,
  calculateAgreement,
  calculateSignature,
  verifySignature,
  generateSignedPreKey,
  generatePreKey,
  generateIdentityKeyPair,
  generateRegistrationId,
} from "../pkg/whatsapp_rust_bridge.js";

// Binary encoding
export { encodeNode, decodeNode } from "../pkg/whatsapp_rust_bridge.js";
export { getWAConnHeader } from "../pkg/whatsapp_rust_bridge.js";

// Crypto
export { md5, hkdf } from "../pkg/whatsapp_rust_bridge.js";
