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
  ContactUpdated,
  ContactNumberChanged,
  ContactSyncRequested,
  ContactUpdate,
  PushNameUpdate,
  PinUpdate,
  MuteUpdate,
  ArchiveUpdate,
  StarUpdate,
  MarkChatAsReadUpdate,
  UserAboutUpdate,
} from "./generated.js";

// Re-export all generated types
export * from "./generated.js";

// ---------------------------------------------------------------------------
// Fully typed WhatsApp event — overrides the wasm-bindgen generated version
// with precise data types from generated.d.ts
// ---------------------------------------------------------------------------

export type WhatsAppEvent =
  | { type: 'connected'; data: Record<string, never> }
  | { type: 'disconnected'; data: Record<string, never> }
  | { type: 'qr'; data: { code: string; timeout: number } }
  | { type: 'pairing_code'; data: { code: string; timeout: number } }
  | { type: 'pair_success'; data: { id: string; lid: string; business_name: string; platform: string } }
  | { type: 'pair_error'; data: { id: string; lid: string; business_name: string; platform: string; error: string } }
  | { type: 'logged_out'; data: { on_connect: boolean; reason: string } }
  | { type: 'message'; data: { message: Record<string, unknown>; info: MessageInfo } }
  | { type: 'receipt'; data: Receipt }
  | { type: 'undecryptable_message'; data: UndecryptableMessage }
  | { type: 'notification'; data: Record<string, unknown> }
  | { type: 'chat_presence'; data: ChatPresenceUpdate }
  | { type: 'presence'; data: PresenceUpdate }
  | { type: 'picture_update'; data: PictureUpdate }
  | { type: 'user_about_update'; data: UserAboutUpdate }
  | { type: 'contact_updated'; data: ContactUpdated }
  | { type: 'contact_number_changed'; data: ContactNumberChanged }
  | { type: 'contact_sync_requested'; data: ContactSyncRequested }
  | { type: 'joined_group'; data: Record<string, unknown> }
  | { type: 'group_update'; data: GroupUpdate }
  | { type: 'contact_update'; data: ContactUpdate }
  | { type: 'push_name_update'; data: PushNameUpdate }
  | { type: 'self_push_name_updated'; data: SelfPushNameUpdated }
  | { type: 'pin_update'; data: PinUpdate }
  | { type: 'mute_update'; data: MuteUpdate }
  | { type: 'archive_update'; data: ArchiveUpdate }
  | { type: 'star_update'; data: StarUpdate }
  | { type: 'mark_chat_as_read_update'; data: MarkChatAsReadUpdate }
  | { type: 'history_sync'; data: Record<string, unknown> }
  | { type: 'offline_sync_preview'; data: OfflineSyncPreview }
  | { type: 'offline_sync_completed'; data: OfflineSyncCompleted }
  | { type: 'device_list_update'; data: DeviceListUpdate }
  | { type: 'business_status_update'; data: BusinessStatusUpdate }
  | { type: 'stream_replaced'; data: Record<string, never> }
  | { type: 'temporary_ban'; data: TemporaryBan }
  | { type: 'connect_failure'; data: ConnectFailure }
  | { type: 'stream_error'; data: StreamError }
  | { type: 'disappearing_mode_changed'; data: DisappearingModeChanged }
  | { type: 'newsletter_live_update'; data: NewsletterLiveUpdate }
  | { type: 'qr_scanned_without_multidevice'; data: Record<string, never> }
  | { type: 'client_outdated'; data: Record<string, never> };

// ---------------------------------------------------------------------------
// Storage interface
// ---------------------------------------------------------------------------

/** JS storage callbacks for persistent backend (file, SQLite, Redis, etc.). */
export interface JsStoreCallbacks {
  /** Get a value from the store. Returns null if not found. */
  get(store: string, key: string): Promise<Uint8Array | null>;
  /** Set a value in the store. */
  set(store: string, key: string, value: Uint8Array): Promise<void>;
  /** Delete a value from the store. */
  delete(store: string, key: string): Promise<void>;
}

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
// Cache Configuration
// ---------------------------------------------------------------------------

/** Custom cache backend (Redis, Memcached, IndexedDB, etc.) */
export interface JsCacheStore {
  get(namespace: string, key: string): Promise<Uint8Array | null>;
  set(namespace: string, key: string, value: Uint8Array, ttlSecs?: number): Promise<void>;
  delete(namespace: string, key: string): Promise<void>;
  clear(namespace: string): Promise<void>;
}

/** Config for a single cache — TTL, capacity, and optional custom store. */
export interface CacheEntryConfig {
  /** Time-to-live in seconds. Omit → keep default. */
  ttlSecs?: number;
  /** Max entries. 0 = disabled. Omit → keep default. */
  capacity?: number;
  /** Custom store for this specific cache. Omit → in-memory. */
  store?: JsCacheStore;
}

/**
 * Cache configuration. All fields optional — omitted values keep defaults.
 *
 * @example
 * // Override just group cache TTL, everything else stays default
 * { group: { ttlSecs: 7200 } }
 *
 * @example
 * // Group cache on Redis, everything else in-memory
 * { group: { store: redisCacheStore } }
 *
 * @example
 * // All pluggable caches on Redis
 * { store: redisCacheStore }
 *
 * @example
 * // Mix: global Redis + custom group TTL
 * { store: redisCacheStore, group: { ttlSecs: 7200, capacity: 1000 } }
 */
export interface CacheConfig {
  /** Global store for all pluggable caches. Per-cache `store` overrides this. */
  store?: JsCacheStore;
  /** Group metadata cache. Default: 1h TTL, 250 entries. */
  group?: CacheEntryConfig;
  /** Device list cache. Default: 1h TTL, 5000 entries. */
  device?: CacheEntryConfig;
  /** Device registry cache. Default: 1h TTL, 5000 entries. */
  deviceRegistry?: CacheEntryConfig;
  /** LID-to-phone mapping cache. Default: 1h timeout, 10000 entries. */
  lidPn?: CacheEntryConfig;
  /** Retried group messages tracker. Default: 5m TTL, 2000 entries. */
  retriedGroupMessages?: CacheEntryConfig;
  /** Recent sent messages cache (for retry). Default: 5m TTL, 0 (disabled). */
  recentMessages?: CacheEntryConfig;
  /** Message retry counter cache. Default: 5m TTL, 1000 entries. */
  messageRetry?: CacheEntryConfig;
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
  /** Enable or disable automatic reconnection (enabled by default). */
  setAutoReconnect(enabled: boolean): void;
  /** Check if WebSocket is currently connected. */
  isConnected(): boolean;
  /** Check if the client has completed pairing. */
  isLoggedIn(): boolean;

  /**
   * Request a pairing code for phone number login (alternative to QR).
   * @param phoneNumber Phone number (e.g. "15551234567")
   * @param customCode Optional custom 8-character code
   * @returns The 8-character pairing code to enter on the phone
   */
  requestPairingCode(phoneNumber: string, customCode?: string): Promise<string>;

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

  /**
   * Send a message from protobuf binary bytes.
   * Avoids serde issues with prost's strict field requirements.
   * @param jid Recipient JID
   * @param messageBytes Protobuf-encoded wa.Message bytes
   * @returns Message ID string
   */
  sendMessageBytes(jid: string, messageBytes: Uint8Array): Promise<string>;

  // ── Message management ──

  /** Edit a previously sent message. Returns new message ID. */
  editMessage(jid: string, messageId: string, newContent: Record<string, unknown>): Promise<string>;
  /** Edit a previously sent message from protobuf binary bytes. Returns new message ID. */
  editMessageBytes(jid: string, messageId: string, newContentBytes: Uint8Array): Promise<string>;
  /** Revoke (delete) a message. Pass participant for admin revoke in groups. */
  revokeMessage(jid: string, messageId: string, participant?: string | null): Promise<void>;

  // ── Groups ──

  /** Update a group's subject (name). */
  groupUpdateSubject(jid: string, subject: string): Promise<void>;
  /** Update a group's description. Pass null to remove. */
  groupUpdateDescription(jid: string, description?: string | null): Promise<void>;
  /** Leave a group. */
  groupLeave(jid: string): Promise<void>;
  /** Update group participants. Action: "add", "remove", "promote", "demote". */
  groupParticipantsUpdate(jid: string, participants: string[], action: "add" | "remove" | "promote" | "demote"): Promise<ParticipantChangeResult[] | void>;
  /** Fetch all groups the user is participating in. Returns map of JID → GroupMetadataResult. */
  groupFetchAllParticipating(): Promise<Record<string, GroupMetadataResult>>;
  /** Get the invite link/code for a group. */
  groupInviteCode(jid: string): Promise<string>;
  /** Update a group setting. Setting: "locked", "announce", "membership_approval". */
  groupSettingUpdate(jid: string, setting: "locked" | "announce" | "membership_approval", value: boolean): Promise<void>;
  /** Set disappearing messages timer (0 to disable). */
  groupToggleEphemeral(jid: string, expiration: number): Promise<void>;
  /** Revoke a group's invite link (returns new code). */
  groupRevokeInvite(jid: string): Promise<string>;
  /** Join a group using an invite code. Returns group JID. */
  groupAcceptInvite(code: string): Promise<string | undefined>;
  /** Get group info from an invite code (without joining). */
  groupGetInviteInfo(code: string): Promise<GroupMetadataResult>;
  /** Get list of pending join requests for a group. */
  groupRequestParticipantsList(jid: string): Promise<Array<{ jid: string; request_time?: string }>>;
  /** Approve or reject pending join requests. */
  groupRequestParticipantsUpdate(jid: string, participants: string[], action: 'approve' | 'reject'): Promise<void>;

  // ── Read receipts ──

  /** Mark messages as read. */
  readMessages(keys: Array<{ remoteJid: string; id: string; participant?: string }>): Promise<void>;

  // ── Privacy ──

  /** Fetch all privacy settings as a key-value map. */
  fetchPrivacySettings(): Promise<Record<string, string>>;
  /** Update a single privacy setting by category name and value. */
  updatePrivacySetting(category: string, value: string): Promise<void>;
  /** Set default disappearing messages duration (seconds). 0 to disable. */
  updateDefaultDisappearingMode(duration: number): Promise<void>;

  // ── Calls ──

  /** Reject an incoming call. */
  rejectCall(callId: string, callFrom: string): Promise<void>;

  // ── User status ──

  /** Fetch user status/about text for one or more JIDs. */
  fetchStatus(jids: string[]): Promise<Array<{ jid: string; status?: string }>>;

  // ── Group member add mode ──

  /** Set who can add members: "admin_add" or "all_member_add". */
  groupMemberAddMode(jid: string, mode: 'admin_add' | 'all_member_add'): Promise<void>;

  // ── Business profile ──

  /** Get business profile information for a JID. */
  getBusinessProfile(jid: string): Promise<{
    wid?: string;
    description: string;
    email?: string;
    website: string[];
    category?: string;
    address?: string;
    business_hours: {
      timezone?: string;
      business_config?: Array<{
        day_of_week: string;
        mode: string;
        open_time?: string;
        close_time?: string;
      }>;
    };
  } | undefined>;

  // ── Message history ──

  /** Request on-demand message history from primary phone. Returns request message ID. */
  fetchMessageHistory(count: number, chatJid: string, oldestMsgId: string, oldestMsgFromMe: boolean, oldestMsgTimestampMs: number): Promise<string>;

  // ── Media ──

  /** Get media connection info (auth token + upload hosts) for media upload/download. */
  getMediaConn(force: boolean): Promise<MediaConnResult>;

  // ── Version ──

  /** Override the WA Web version used for connection. */
  setVersion(major: number, minor: number, patch: number): void;

  // ── Device props ──

  /**
   * Set device properties (OS name, browser/platform type).
   * Controls what device name is shown on the phone.
   * @param os OS name (e.g. "Mac OS", "Windows", "Ubuntu")
   * @param browser Browser name (e.g. "Chrome", "Firefox", "Safari", "Edge", "Desktop")
   */
  setDeviceProps(os: string, browser: string): Promise<void>;

  // ── Media reupload ──

  /**
   * Request the server to re-upload expired media.
   * Returns the new directPath on success, throws on failure.
   */
  requestMediaReupload(
    msgId: string,
    chatJid: string,
    mediaKey: Uint8Array,
    isFromMe: boolean,
    participant?: string | null,
  ): Promise<string>;

  // ── Contacts ──

  /** Get profile picture URL. Type: "preview" (thumbnail) or "image" (full). */
  profilePictureUrl(jid: string, type: "preview" | "image"): Promise<ProfilePictureResult | null>;
  /** Fetch user info for one or more JIDs. */
  fetchUserInfo(jids: string[]): Promise<Record<string, UserInfoResult>>;

  // ── Profile ──

  /** Set the user's display name. */
  setPushName(name: string): Promise<void>;
  /** Set profile picture. Accepts raw image bytes. */
  updateProfilePicture(imgData: Uint8Array): Promise<{ id: string }>;
  /** Remove profile picture. */
  removeProfilePicture(): Promise<{ id: string }>;
  /** Update status text (about). */
  updateProfileStatus(status: string): Promise<void>;

  // ── Blocking ──

  /** Block or unblock a contact. */
  updateBlockStatus(jid: string, action: "block" | "unblock"): Promise<void>;
  /** Fetch the full blocklist. */
  fetchBlocklist(): Promise<BlocklistResult[]>;

  // ── Chat actions ──

  /** Pin or unpin a chat. */
  pinChat(jid: string, pin: boolean): Promise<void>;
  /** Mute a chat until a timestamp (ms), or pass null to unmute. */
  muteChat(jid: string, muteUntil?: number | null): Promise<void>;
  /** Archive or unarchive a chat. */
  archiveChat(jid: string, archive: boolean): Promise<void>;
  /** Star or unstar a message. */
  starMessage(jid: string, messageId: string, star: boolean): Promise<void>;

  // ── Presence ──

  /** Send presence ("available" or "unavailable"). */
  sendPresence(status: "available" | "unavailable"): Promise<void>;
  /** Subscribe to a contact's presence updates. */
  presenceSubscribe(jid: string): Promise<void>;
  /** Send typing indicator. */
  sendChatState(jid: string, state: "composing" | "recording" | "paused"): Promise<void>;

  // ── Newsletter ──

  /** Create a new newsletter (channel). */
  newsletterCreate(name: string, description?: string | null): Promise<NewsletterMetadataResult>;
  /** Fetch newsletter metadata by JID. */
  newsletterMetadata(jid: string): Promise<NewsletterMetadataResult>;
  /** Subscribe (join) a newsletter. */
  newsletterSubscribe(jid: string): Promise<NewsletterMetadataResult>;
  /** Unsubscribe (leave) a newsletter. */
  newsletterUnsubscribe(jid: string): Promise<void>;

  /** Free WASM resources. Call when done with the client. */
  free(): void;
}

/** Result from getMediaConn. */
export interface MediaConnResult {
  auth: string;
  ttl: number;
  hosts: Array<{ hostname: string; maxContentLengthBytes?: number }>;
  fetchDate: Date;
}

/** Result from groupParticipantsUpdate (add/remove). */
export interface ParticipantChangeResult {
  jid: string;
  status?: string | null;
  error?: string | null;
}

/** Result from profilePictureUrl. */
export interface ProfilePictureResult {
  id: string;
  url: string;
  directPath?: string | null;
  hash?: string | null;
}

/** Result from fetchUserInfo (per-JID). */
export interface UserInfoResult {
  jid: string;
  lid?: string | null;
  status?: string | null;
  pictureId?: string | null;
  isBusiness: boolean;
}

/** Result from fetchBlocklist. */
export interface BlocklistResult {
  jid: string;
  timestamp?: number | null;
}

/** Result from newsletter methods. */
export interface NewsletterMetadataResult {
  jid: string;
  name: string;
  description?: string | null;
  subscriberCount: number;
  verification: string;
  state: string;
  pictureUrl?: string | null;
  previewUrl?: string | null;
  inviteCode?: string | null;
  role?: string | null;
  creationTime?: number | null;
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

/** Initialize the WASM engine. Call once before creating clients.
 * @param logger Optional pino-compatible logger. If provided, all Rust logs route through it.
 *               If omitted, falls back to console.log with "warn" level.
 */
export declare function initWasmEngine(logger?: { level: string; trace: Function; debug: Function; info: Function; warn: Function; error: Function }): void;

/**
 * Create a full WhatsApp client running in WASM.
 *
 * @param transport WebSocket transport callbacks
 * @param httpClient HTTP client callbacks (for media, version fetching)
 * @param onEvent Event callback — receives typed WhatsApp events
 * @param store Optional JS storage callbacks — if provided, enables persistent storage
 */
export declare function createWhatsAppClient(
  transport: JsTransportCallbacks,
  httpClient: JsHttpClientConfig,
  onEvent?: ((event: WhatsAppEvent) => void) | null,
  store?: JsStoreCallbacks | null,
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
