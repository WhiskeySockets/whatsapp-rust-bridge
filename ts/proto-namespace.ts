/**
 * Auto-assembled `proto` namespace, protobufjs-API-compatible.
 *
 * Walks every value exported by `./generated/whatsapp` (the ts-proto output),
 * splits the flat `Parent_Child_GrandChild` naming into a nested namespace tree,
 * and wraps each `MessageFns` into the `{ encode, decode, fromObject, create,
 * toObject }` surface that legacy Baileys-style consumers expect from
 * `WAProto.X.encode(obj).finish()` and friends.
 *
 * Why: there used to be a hand-maintained shim covering ~90 of the 1,352
 * generated types. Bots that touched anything outside the manual list
 * type-checked against `WAProto/index.d.ts` but crashed at runtime. Generating
 * the runtime here from the same ts-proto output the bridge already builds
 * eliminates that drift entirely.
 *
 * Conventions matched:
 * - `encode(obj).finish()` returns `Uint8Array` (ts-proto's `BinaryWriter`
 *   already exposes `.finish()`, so we reuse it directly).
 * - `decode(bytes)` returns the typed object with a no-op `toJSON` so
 *   `JSON.stringify` round-trips cleanly without protobufjs's bytes→base64
 *   conversion (matches baileyrs' previous shim behavior).
 * - `fromObject(obj)` / `create(obj)` accept partial input and return the same
 *   shape (no normalization — ts-proto's runtime accepts plain objects).
 * - Enums are flattened into the namespace as plain `{ NAME: value }` objects.
 * - Each top-level message is wrapped in a `Proxy` so unknown capitalized
 *   sub-properties (`proto.Message.SomeUnreleasedThing.fromObject({...})`)
 *   synthesize a passthrough at access time, preventing TypeError on bot code
 *   that compiled against an updated `.d.ts` but runs against an older runtime.
 */

import * as gen from "./generated/whatsapp";
import { encodeProto, decodeProto } from "./proto";

// Tag the union type loosely — the generated module is huge and individual
// member types matter less than the shape we extract from each value.
type AnyExport = unknown;

interface MessageFnsLike {
  encode: (msg: any, writer?: any) => any;
  decode: (input: any, length?: number) => any;
  fromPartial?: (obj: any) => any;
  create?: (obj: any) => any;
}

const isMessageFns = (v: AnyExport): v is MessageFnsLike =>
  typeof v === "object" &&
  v !== null &&
  typeof (v as MessageFnsLike).encode === "function" &&
  typeof (v as MessageFnsLike).decode === "function";

// Enums in ts-proto come out as plain objects whose values are numbers (and
// reverse-mappings for numeric enums). We treat any non-MessageFns object
// export as an enum — there's nothing else generated at this layer.
const isEnumLike = (v: AnyExport): v is Record<string, number | string> =>
  typeof v === "object" && v !== null && !isMessageFns(v);

const attachToJSONIdentity = (obj: unknown): void => {
  if (obj && typeof obj === "object" && !("toJSON" in obj)) {
    Object.defineProperty(obj, "toJSON", {
      value: function () {
        return this;
      },
      enumerable: false,
      writable: true,
      configurable: true,
    });
  }
};

const wrapMessage = (typeName: string, fns: MessageFnsLike) => ({
  encode(obj: any) {
    return {
      finish(): Uint8Array {
        return encodeProto(typeName, obj);
      },
    };
  },
  decode(buffer: Uint8Array | ArrayBuffer | ArrayBufferView): any {
    const data =
      buffer instanceof Uint8Array
        ? buffer
        : ArrayBuffer.isView(buffer)
          ? new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength)
          : new Uint8Array(buffer as ArrayBuffer);
    const decoded = decodeProto(typeName, data);
    attachToJSONIdentity(decoded);
    return decoded;
  },
  create(obj?: any) {
    return obj || {};
  },
  fromObject(obj?: any) {
    if (obj && typeof obj === "object") attachToJSONIdentity(obj);
    return obj || {};
  },
  toObject(obj: any) {
    return obj;
  },
  // Expose ts-proto's fromPartial for callers that prefer it / for the
  // namespace shim's own bookkeeping.
  fromPartial: fns.fromPartial?.bind(fns) ?? ((obj: any) => obj || {}),
});

const passthrough = () => ({
  create(obj?: any) {
    return obj || {};
  },
  fromObject(obj?: any) {
    return obj || {};
  },
});

/**
 * Wrap an object so unknown capitalized accesses synthesize a passthrough.
 * Lets `proto.Message.NewlyAddedThing.fromObject(...)` keep working when bots
 * compile against a newer `.d.ts` than the runtime knows about.
 */
const wrapWithLazyChildren = <T extends object>(target: T): T =>
  new Proxy(target, {
    get(t, prop, receiver) {
      const value = Reflect.get(t, prop, receiver);
      if (value !== undefined) return value;
      if (typeof prop !== "string" || !/^[A-Z]/.test(prop)) return value;
      const synth = passthrough();
      Reflect.set(t, prop, synth);
      return synth;
    },
  });

// ts-proto names nested types like `Message_VideoMessage` and nested enums like
// `Message_ProtocolMessage_Type`. Splitting on `_` gives the namespace path
// because proto type names themselves never contain underscores by convention.
const splitNamespacePath = (flatName: string): string[] => flatName.split("_");

// Some message-class names need protobufjs-style aliases that don't fall out
// of the underscore-split scheme. Add them here as needed — `ADV*` is the
// classic case (proto has `ADVDeviceIdentity`; ts-proto preserves it; both
// shapes are accepted by upstream Baileys consumers).
const HISTORICAL_ALIASES: Record<string, string> = {
  ADVKeyIndexList: "ADVSignedKeyIndexList",
};

const buildNamespace = (): Record<string, any> => {
  const root: Record<string, any> = {};

  for (const [flatName, value] of Object.entries(gen)) {
    if (flatName === "protobufPackage" || flatName.startsWith("_")) continue;
    if (typeof value === "function") continue; // skip ts-proto's tag helpers

    const path = splitNamespacePath(flatName);
    const leaf = path[path.length - 1]!;
    let cursor = root;
    for (let i = 0; i < path.length - 1; i++) {
      const segment = path[i]!;
      if (!cursor[segment]) cursor[segment] = {};
      cursor = cursor[segment];
    }

    if (isMessageFns(value)) {
      // Preserve any nested namespace already attached at this slot
      // (children are processed in any order).
      const wrapped = wrapMessage(flatName.replace(/_/g, "."), value);
      cursor[leaf] = Object.assign(wrapped, cursor[leaf] || {});
    } else if (isEnumLike(value)) {
      // Merge enum entries onto whatever might already be there.
      cursor[leaf] = Object.assign({}, cursor[leaf] || {}, value);
    }
  }

  for (const [alias, target] of Object.entries(HISTORICAL_ALIASES)) {
    if (root[target] && !root[alias]) root[alias] = root[target];
  }

  // Wrap the busiest top-level messages so unknown sub-types lazy-fall through.
  // Limit to the well-known carriers — wrapping every node would be overkill.
  for (const lazyParent of ["Message", "WebMessageInfo", "ContextInfo", "SyncActionValue"]) {
    if (root[lazyParent]) root[lazyParent] = wrapWithLazyChildren(root[lazyParent]);
  }

  return root;
};

/**
 * Protobufjs-shaped namespace covering every type the bridge knows about.
 * Stable surface; safe to import as `proto` (or alias to `WAProto` for legacy
 * upstream-Baileys-style bot code).
 */
export const proto: Record<string, any> = buildNamespace();
