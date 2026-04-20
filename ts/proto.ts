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

export function encodeProto(typeName: string, obj: unknown): Uint8Array {
  const fns = resolve(typeName);
  return fns.encode(fns.fromPartial(obj ?? {})).finish();
}

export function decodeProto(typeName: string, data: Uint8Array): unknown {
  const fns = resolve(typeName);
  return fns.decode(data);
}
