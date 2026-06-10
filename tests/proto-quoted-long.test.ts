/**
 * encodeProto must accept protobufjs-style `Long` objects ({ low, high,
 * unsigned }) in i64/u64 fields. The camel serializer emits them for decoded
 * incoming messages, so they reappear when a decoded message is re-encoded —
 * e.g. `contextInfo.quotedMessage` when sending a quoted reply.
 *
 * Run: bun test tests/proto-quoted-long.test.ts (requires bun run build)
 */
import { describe, test, expect, beforeAll } from "bun:test";
import { initWasmEngine, encodeProto, decodeProto } from "../dist/index.js";

beforeAll(() => {
  initWasmEngine();
});

const TS_SMALL = 1_700_000_123; // fits in the low half
const TS_BIG = 9_007_199_254_740_993n; // 2^53 + 1: not representable as a double

/** Split a value into protobufjs Long parts (two's complement for negatives). */
function longOf(v: bigint, unsigned: boolean) {
  const u = v < 0n ? (1n << 64n) + v : v;
  return {
    low: Number(u & 0xffffffffn) | 0,
    high: Number((u >> 32n) & 0xffffffffn) | 0,
    unsigned,
  };
}

const quotedReplyMsg = (mediaKeyTimestamp: unknown, ephemeralSettingTimestamp: unknown) => ({
  extendedTextMessage: {
    text: "reply",
    contextInfo: {
      stanzaId: "ABCDEF123",
      participant: "5521999999999@s.whatsapp.net",
      ephemeralSettingTimestamp,
      quotedMessage: {
        videoMessage: {
          url: "https://mmg.whatsapp.net/d/f/x",
          mediaKey: new Uint8Array(32).fill(7),
          mediaKeyTimestamp,
          seconds: 12,
        },
      },
    },
  },
});

describe("encodeProto with protobufjs Long objects (quoted reply path)", () => {
  test("encodes and round-trips exact values", () => {
    const msg = quotedReplyMsg(longOf(BigInt(TS_SMALL), false), longOf(BigInt(TS_SMALL), false));
    const bytes = encodeProto("Message", msg);
    const decoded: any = decodeProto("Message", bytes);
    expect(decoded.extendedTextMessage.contextInfo.quotedMessage.videoMessage.mediaKeyTimestamp)
      .toBe(TS_SMALL);
    expect(decoded.extendedTextMessage.contextInfo.ephemeralSettingTimestamp).toBe(TS_SMALL);
  });

  test("negative i64 Long (two's-complement high) encodes correctly", () => {
    const msg = quotedReplyMsg(longOf(-1n, false), longOf(-2n, false));
    const bytes = encodeProto("Message", msg);
    const decoded: any = decodeProto("Message", bytes);
    expect(decoded.extendedTextMessage.contextInfo.quotedMessage.videoMessage.mediaKeyTimestamp)
      .toBe(-1);
    expect(decoded.extendedTextMessage.contextInfo.ephemeralSettingTimestamp).toBe(-2);
  });

  test("beyond-2^53 Long encodes without precision loss (encode path)", () => {
    // decode throws longToNumber MAX_SAFE_INTEGER (pre-existing ts-proto limit),
    // so verify the encoded bytes against a known-good encode from a bigint.
    const bytes = encodeProto("Message", quotedReplyMsg(longOf(TS_BIG, false), undefined));
    const ref = encodeProto("Message", quotedReplyMsg(TS_BIG, undefined));
    expect(Buffer.from(bytes).toString("hex")).toBe(Buffer.from(ref).toString("hex"));
  });

  test("input object is not mutated (Long objects survive on the original)", () => {
    const long = longOf(BigInt(TS_SMALL), false);
    // Snapshot before encoding: `kept` is the same reference as `long`, so
    // comparing kept fields to long fields would pass even after an in-place
    // mutation — only a pre-encode copy catches that.
    const snapshot = { ...long };
    const msg = quotedReplyMsg(long, undefined);
    encodeProto("Message", msg);
    const kept = (msg.extendedTextMessage.contextInfo.quotedMessage.videoMessage as any)
      .mediaKeyTimestamp;
    expect(kept).toBe(long);
    expect(kept).toEqual(snapshot);
  });

  test("clean message without Longs still encodes (fast path)", () => {
    const bytes = encodeProto("Message", { conversation: "hi" });
    const decoded: any = decodeProto("Message", bytes);
    expect(decoded.conversation).toBe("hi");
  });

  test("byte fields (Uint8Array) are passed through untouched", () => {
    const msg = quotedReplyMsg(longOf(BigInt(TS_SMALL), false), undefined);
    const bytes = encodeProto("Message", msg);
    const decoded: any = decodeProto("Message", bytes);
    expect(
      Array.from(decoded.extendedTextMessage.contextInfo.quotedMessage.videoMessage.mediaKey),
    ).toEqual(new Array(32).fill(7));
  });
});
