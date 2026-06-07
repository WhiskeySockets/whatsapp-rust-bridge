import {
  Message,
  WebMessageInfo,
  HistorySync,
  SyncActionData,
  ClientPayload,
  ADVSignedDeviceIdentity,
  ADVSignedKeyIndexList,
  ADVDeviceIdentity,
  ADVSignedDeviceIdentityHMAC,
  HandshakeMessage,
  SyncdRecord,
  SyncdMutation,
  SyncdMutations,
  SyncdPatch,
  SyncdSnapshot,
  ExitCode,
  SyncActionValue,
  DeviceProps,
  SenderKeyDistributionMessage,
  SenderKeyMessage,
  ServerErrorReceipt,
  CertChain,
  CertChain_NoiseCertificate,
  CertChain_NoiseCertificate_Details,
  ExternalBlobReference,
  LIDMigrationMappingSyncPayload,
  MediaRetryNotification,
  VerifiedNameCertificate,
  VerifiedNameCertificate_Details,
  Message_PollVoteMessage,
  Message_EventResponseMessage,
} from "./generated/whatsapp";

interface MessageFns<T> {
  encode(message: T, writer?: any): any;
  decode(input: Uint8Array | any, length?: number): T;
  fromPartial(obj: any): T;
}

const REGISTRY: Record<string, MessageFns<any>> = {
  "Message": Message,
  "WebMessageInfo": WebMessageInfo,
  "HistorySync": HistorySync,
  "SyncActionData": SyncActionData,
  "ClientPayload": ClientPayload,
  "AdvSignedDeviceIdentity": ADVSignedDeviceIdentity,
  "AdvSignedKeyIndexList": ADVSignedKeyIndexList,
  "AdvDeviceIdentity": ADVDeviceIdentity,
  "AdvSignedDeviceIdentityHmac": ADVSignedDeviceIdentityHMAC,
  "HandshakeMessage": HandshakeMessage,
  "SyncdRecord": SyncdRecord,
  "SyncdMutation": SyncdMutation,
  "SyncdMutations": SyncdMutations,
  "SyncdPatch": SyncdPatch,
  "SyncdSnapshot": SyncdSnapshot,
  "ExitCode": ExitCode,
  "SyncActionValue": SyncActionValue,
  "DeviceProps": DeviceProps,
  "SenderKeyDistributionMessage": SenderKeyDistributionMessage,
  "SenderKeyMessage": SenderKeyMessage,
  "ServerErrorReceipt": ServerErrorReceipt,
  "CertChain": CertChain,
  "CertChain.NoiseCertificate": CertChain_NoiseCertificate,
  "CertChain.NoiseCertificate.Details": CertChain_NoiseCertificate_Details,
  "ExternalBlobReference": ExternalBlobReference,
  "LidMigrationMappingSyncPayload": LIDMigrationMappingSyncPayload,
  "MediaRetryNotification": MediaRetryNotification,
  "VerifiedNameCertificate": VerifiedNameCertificate,
  "VerifiedNameCertificate.Details": VerifiedNameCertificate_Details,
  "Message.PollVoteMessage": Message_PollVoteMessage,
  "Message.EventResponseMessage": Message_EventResponseMessage,
};

// Star-import the generated module so any ts-proto type is resolvable by name
// without us having to register each manually. Bundled at build time (bun
// includes all imports), so the runtime cost is one extra Object.entries-style
// lookup on the cold path.
import * as gen from "./generated/whatsapp";

const GENERATED_MODULE = gen as unknown as Record<string, unknown>;

function resolve(typeName: string): MessageFns<any> {
  // Hot path: the small REGISTRY of well-known top-level types above.
  const direct = REGISTRY[typeName];
  if (direct) return direct;
  // Fallback: protobufjs-style namespace path (e.g. `Message.VideoMessage`)
  // is mapped to ts-proto's flat `Message_VideoMessage` and looked up in the
  // generated module. Any wacore proto type the bridge knows about resolves
  // here, no manual registration needed.
  const flatName = typeName.replace(/\./g, "_");
  const candidate = GENERATED_MODULE[flatName];
  if (candidate && typeof candidate === "object" && "encode" in candidate) {
    return candidate as MessageFns<any>;
  }
  throw new Error(`unknown proto type: ${typeName}`);
}

// The bridge serializes protobuf 64-bit fields (int64/uint64/sfixed64/…) as
// protobufjs-style `Long` objects `{ low, high, unsigned }` (see
// `src/camel_serializer.rs`) so consumers can `JSON.stringify` events without
// the BigInt serialization error. But the ts-proto encoder (`@bufbuild/protobuf`)
// only accepts `number | bigint | string` for those fields — handed a `Long`
// *object* it does `BigInt(obj)`, which throws. This bites when re-encoding a
// message that embeds a decoded message, e.g. `contextInfo.quotedMessage` from a
// quoted reply (`sendMessage(..., { quoted })`), whose nested i64 fields are
// still `Long` objects. Normalize them to precision-safe BigInt before encoding.
// `unsigned` is the discriminant (matches the serializer's own Long detection),
// so plain `{ low, high }` data objects are left untouched.
type LongObject = { low: number; high: number; unsigned: boolean };

function isLongObject(v: unknown): v is LongObject {
  // `unsigned` (a boolean) is the discriminant: testing it first short-circuits
  // virtually every non-Long object in one comparison, and matches the guard in
  // camel_serializer.rs — a plain `{ low, high }` data object without `unsigned`
  // is NOT a Long and is left untouched.
  return (
    typeof v === "object" &&
    v !== null &&
    typeof (v as LongObject).unsigned === "boolean" &&
    typeof (v as LongObject).low === "number" &&
    typeof (v as LongObject).high === "number"
  );
}

function longToBigInt(l: LongObject): bigint {
  const lo = BigInt(l.low >>> 0);
  const hi = l.unsigned ? BigInt(l.high >>> 0) : BigInt(l.high | 0);
  return hi * 4294967296n + lo;
}

// Replace every nested `Long` object with a BigInt, allocation-consciously:
//  - Zero allocation on the clean path: a message with no Long objects (the
//    common non-quoted send) is returned by reference, untouched — no GC churn.
//  - Structural sharing: only the objects/arrays ON THE PATH to a Long are
//    copied (`{ ...obj }` / `slice()`); unchanged subtrees are shared by ref.
//  - Never mutates the input (the locally-echoed message keeps its Long objects).
//  - Short-circuits on primitives and `Uint8Array`/typed-array byte fields.
// Runs once per `encodeProto` — the outgoing-message path, not a hot receive loop.
function normalizeLongObjects(value: unknown): unknown {
  if (isLongObject(value)) return longToBigInt(value);
  if (value === null || typeof value !== "object") return value;
  if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer) return value;
  if (Array.isArray(value)) {
    let copy: unknown[] | undefined;
    for (let i = 0; i < value.length; i++) {
      const nv = normalizeLongObjects(value[i]);
      if (nv !== value[i]) (copy ??= value.slice())[i] = nv;
    }
    return copy ?? value;
  }
  const obj = value as Record<string, unknown>;
  let copy: Record<string, unknown> | undefined;
  // `for...in` (not `Object.keys`) is deliberate: it visits keys WITHOUT
  // allocating a keys array, keeping the clean path zero-allocation. ts-proto
  // message objects are plain (no enumerable prototype props), so nothing extra
  // is visited.
  for (const k in obj) {
    const nv = normalizeLongObjects(obj[k]);
    if (nv !== obj[k]) (copy ??= { ...obj })[k] = nv;
  }
  return copy ?? value;
}

export function encodeProto(typeName: string, obj: unknown): Uint8Array {
  const fns = resolve(typeName);
  return fns.encode(fns.fromPartial(normalizeLongObjects(obj ?? {}))).finish();
}

export function decodeProto(typeName: string, data: Uint8Array): unknown {
  const fns = resolve(typeName);
  return fns.decode(data);
}
