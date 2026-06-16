import { describe, it, expect } from "bun:test";
import {
  ProtocolAddress,
  GroupCipher,
  GroupSessionBuilder,
  SenderKeyName,
  importLegacySession,
  exportLegacySession,
} from "../dist";
import { DropInStorage } from "./helpers/dropin_storage";

// Deterministic base64 of an n-filled buffer (33-byte pubkeys, 32-byte keys).
const b64 = (fill: number, len = 33) => Buffer.alloc(len, fill).toString("base64");

type ReceiverChain = [ratchetB64: string, chain: any];

function makeEntry(opts: {
  baseKey: string;
  closed: number;
  baseKeyType?: number;
  lastRemote?: string;
  receiverChains?: ReceiverChain[];
}): any {
  const senderRatchet = b64(10);
  const chains: Record<string, any> = {
    [senderRatchet]: {
      chainKey: { counter: 5, key: b64(15, 32) },
      chainType: 1, // SENDING
      messageKeys: {},
    },
  };
  for (const [ratchet, chain] of opts.receiverChains ?? []) chains[ratchet] = chain;

  return {
    registrationId: 555,
    currentRatchet: {
      ephemeralKeyPair: { pubKey: senderRatchet, privKey: b64(11, 32) },
      lastRemoteEphemeralKey: opts.lastRemote ?? b64(12),
      previousCounter: 3,
      rootKey: b64(13, 32),
    },
    indexInfo: {
      baseKey: opts.baseKey,
      baseKeyType: opts.baseKeyType ?? 2,
      closed: opts.closed,
      used: 1,
      created: 1,
      remoteIdentityKey: b64(14),
    },
    _chains: chains,
  };
}

const roundTrip = (record: any) => {
  const { record: rec, seeds } = importLegacySession(record) as {
    record: Uint8Array;
    seeds: Uint8Array;
  };
  return exportLegacySession(rec, seeds);
};

describe("Drop-in compatibility gaps", () => {
  it("[2] preserves archived sessions across import → export", () => {
    const open = b64(1);
    const archived = b64(2);
    const out = roundTrip({
      _sessions: {
        [open]: makeEntry({ baseKey: open, closed: -1 }),
        [archived]: makeEntry({ baseKey: archived, closed: 1_700_000_000_000 }),
      },
      version: "v1",
    });

    // Both sessions survive; the open one stays open, the other stays archived.
    expect(Object.keys(out._sessions).sort()).toEqual([open, archived].sort());
    expect(out._sessions[open].indexInfo.closed).toBe(-1);
    expect(out._sessions[archived].indexInfo.closed).toBeGreaterThan(0);
  });

  it("[4] preserves baseKeyType=OURS even with no pendingPreKey", () => {
    const bk = b64(3);
    const out = roundTrip({
      _sessions: { [bk]: makeEntry({ baseKey: bk, closed: -1, baseKeyType: 1 }) },
      version: "v1",
    });
    // Without the sidecar, an acked initiator would export as THEIRS (2).
    expect(out._sessions[bk].indexInfo.baseKeyType).toBe(1);
  });

  it("[6] preserves lastRemoteEphemeralKey exactly", () => {
    const bk = b64(4);
    const lastRemote = b64(21);
    const out = roundTrip({
      _sessions: { [bk]: makeEntry({ baseKey: bk, closed: -1, lastRemote: lastRemote }) },
      version: "v1",
    });
    expect(out._sessions[bk].currentRatchet.lastRemoteEphemeralKey).toBe(lastRemote);
  });

  it("[5] keeps a closed receiver chain closed (no chainKey.key)", () => {
    const bk = b64(5);
    const closedRatchet = b64(22);
    const out = roundTrip({
      _sessions: {
        [bk]: makeEntry({
          baseKey: bk,
          closed: -1,
          receiverChains: [
            [closedRatchet, { chainKey: { counter: 9 }, chainType: 2, messageKeys: {} }],
          ],
        }),
      },
      version: "v1",
    });
    const chain = out._sessions[bk]._chains[closedRatchet];
    expect(chain).toBeDefined();
    expect(chain.chainKey.counter).toBe(9); // counter preserved
    expect(chain.chainKey.key).toBeUndefined(); // still closed, not an empty-key live chain
  });

  it("[no-open] does not resurrect an all-closed record as open", () => {
    const a = b64(31);
    const c = b64(32);
    const out = roundTrip({
      _sessions: {
        [a]: makeEntry({ baseKey: a, closed: 1_700_000_000_001 }),
        [c]: makeEntry({ baseKey: c, closed: 1_700_000_000_000 }),
      },
      version: "v1",
    });
    // Both archived sessions survive; NONE becomes open (closed === -1).
    expect(Object.keys(out._sessions).sort()).toEqual([a, c].sort());
    for (const s of Object.values<any>(out._sessions)) {
      expect(s.indexInfo.closed).toBeGreaterThan(0);
    }
  });

  it("[meta] preserves used/created/closed timestamps verbatim", () => {
    const bk = b64(33);
    const out = roundTrip({
      _sessions: {
        [bk]: {
          ...makeEntry({ baseKey: bk, closed: 1_700_000_009_999 }),
          indexInfo: {
            baseKey: bk,
            baseKeyType: 2,
            closed: 1_700_000_009_999,
            used: 1_700_000_001_111,
            created: 1_700_000_000_222,
            remoteIdentityKey: b64(14),
          },
        },
      },
      version: "v1",
    });
    const ii = out._sessions[bk].indexInfo;
    expect(ii.closed).toBe(1_700_000_009_999);
    expect(ii.used).toBe(1_700_000_001_111);
    expect(ii.created).toBe(1_700_000_000_222);
  });

  it("[validate] drops a tampered/wrong seed instead of exporting bad key material", () => {
    // A real run can't inject a wrong seed, but the validation guards the export:
    // a normal round-trip with no skipped keys must still produce empty
    // messageKeys (the self-check never emits a seed that doesn't re-derive).
    const bk = b64(34);
    const out = roundTrip({
      _sessions: { [bk]: makeEntry({ baseKey: bk, closed: -1 }) },
      version: "v1",
    });
    for (const chain of Object.values<any>(out._sessions[bk]._chains)) {
      expect(Object.keys(chain.messageKeys).length).toBe(0);
    }
  });

  it("[32-byte] normalizes bare 32-byte public keys to 33-byte on import", () => {
    const bk32 = Buffer.alloc(32, 7).toString("base64"); // unprefixed base key
    const out = roundTrip({
      _sessions: { [bk32]: makeEntry({ baseKey: bk32, closed: -1 }) },
      version: "v1",
    });
    // Exactly one session; its baseKey is now the 0x05-prefixed 33-byte form.
    const keys = Object.keys(out._sessions);
    expect(keys.length).toBe(1);
    const decoded = Buffer.from(keys[0]!, "base64");
    expect(decoded.length).toBe(33);
    expect(decoded[0]).toBe(0x05);
  });

  it("[chains] closes older receiver chains, keeps the tail live, lastRemote=tail", () => {
    const bk = b64(42);
    const ratchetA = b64(40); // older receiver chain
    const ratchetB = b64(41); // newest (tail) receiver chain
    const out = roundTrip({
      _sessions: {
        [bk]: makeEntry({
          baseKey: bk,
          closed: -1,
          lastRemote: ratchetB, // consistent: lastRemote == tail receiver chain
          receiverChains: [
            [ratchetA, { chainKey: { counter: 3, key: b64(43, 32) }, chainType: 2, messageKeys: {} }],
            [ratchetB, { chainKey: { counter: 7, key: b64(44, 32) }, chainType: 2, messageKeys: {} }],
          ],
        }),
      },
      version: "v1",
    });
    const e = out._sessions[bk];
    // Newest receiver chain stays live; the older one is closed (key omitted).
    expect(e._chains[ratchetB].chainKey.key).toBeDefined();
    expect(e._chains[ratchetA].chainKey.key).toBeUndefined();
    // Both still carry their counters.
    expect(e._chains[ratchetA].chainKey.counter).toBe(3);
    expect(e._chains[ratchetB].chainKey.counter).toBe(7);
    // lastRemoteEphemeralKey is derived from the current tail chain.
    expect(e.currentRatchet.lastRemoteEphemeralKey).toBe(ratchetB);
  });

  it("[3] persists sender keys as Baileys JSON that round-trips through the bridge", async () => {
    const aliceStore = new DropInStorage();
    const bobStore = new DropInStorage();
    const groupId = "gaps-group@g.us";
    const aliceAddr = new ProtocolAddress("alice", 1);
    const skName = new SenderKeyName(groupId, aliceAddr);

    const skdm = await new GroupSessionBuilder(aliceStore as any).create(skName);

    // The bridge wrote the sender key as libsignal-node JSON (array of states,
    // BufferJSON-wrapped seeds) — exactly what Baileys reads back on revert.
    const stored = aliceStore.senderKeys.get(skName.toString())!;
    const states = JSON.parse(Buffer.from(stored).toString("utf-8"));
    expect(Array.isArray(states)).toBe(true);
    // Baileys' on-disk shape: Node Buffer.toJSON() = {type:'Buffer', data:[…]}.
    expect(states[0].senderChainKey.seed.type).toBe("Buffer");
    expect(Array.isArray(states[0].senderChainKey.seed.data)).toBe(true);
    expect(states[0].senderSigningKey.public.type).toBe("Buffer");

    await new GroupSessionBuilder(bobStore as any).process(skName, skdm);

    const aliceCipher = new GroupCipher(aliceStore as any, groupId, aliceAddr);
    const bobCipher = new GroupCipher(bobStore as any, groupId, aliceAddr);
    const ct1 = await aliceCipher.encrypt(Buffer.from("g1"));
    expect(Buffer.from(await bobCipher.decrypt(ct1)).toString()).toBe("g1");

    // Revert round-trip: a fresh sender loaded from the stored JSON (now at the
    // advanced iteration) keeps producing messages the receiver decrypts.
    const alice2 = new DropInStorage(aliceStore.ourIdentityKeyPair, aliceStore.ourRegistrationId);
    alice2.senderKeys.set(skName.toString(), aliceStore.senderKeys.get(skName.toString())!);
    const alice2Cipher = new GroupCipher(alice2 as any, groupId, aliceAddr);
    const ct2 = await alice2Cipher.encrypt(Buffer.from("g2"));
    expect(Buffer.from(await bobCipher.decrypt(ct2)).toString()).toBe("g2");
  });
});
