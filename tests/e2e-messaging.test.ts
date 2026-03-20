/**
 * E2E test: Two WASM clients connect, pair, login, and exchange text messages.
 *
 * Prerequisites:
 *   - Mock server running on wss://127.0.0.1:8080/ws/chat
 *   - Bridge built: bun run build:dev
 *
 * Run: NODE_TLS_REJECT_UNAUTHORIZED=0 bun test tests/e2e-messaging.test.ts
 */

import { describe, test, expect, beforeAll, afterAll } from "bun:test";
import { initWasmEngine, createWhatsAppClient } from "../dist/index.js";
import type {
  WhatsAppEvent,
  WasmWhatsAppClient,
  MessageInfo,
} from "../types/index.js";
import {
  createTransport,
  createHttp,
  waitForEvent,
  waitForEventMatching,
} from "./helpers.js";

process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";

beforeAll(() => {
  initWasmEngine();
});

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

interface TestClient {
  client: WasmWhatsAppClient;
  events: WhatsAppEvent[];
  name: string;
}

async function createTestClient(name: string): Promise<TestClient> {
  const events: WhatsAppEvent[] = [];
  const client = await createWhatsAppClient(
    createTransport(name),
    createHttp(),
    (event) => {
      console.log(`  [${name}] event: ${event.type}`);
      events.push(event);
    }
  );
  return { client, events, name };
}

type MessageEventData = {
  message: Record<string, unknown>;
  info: Record<string, unknown>;
};

function isMessageEvent(
  e: WhatsAppEvent
): e is WhatsAppEvent & { type: "message"; data: MessageEventData } {
  return e.type === "message";
}

/** Get isFromMe from message info source, handling both camelCase and snake_case */
function getIsFromMe(data: MessageEventData): boolean | undefined {
  const source = (data.info?.source ?? data.info) as Record<string, unknown>;
  return (source?.isFromMe ?? source?.is_from_me) as boolean | undefined;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("Two-client E2E messaging", () => {
  let alice: TestClient;
  let bob: TestClient;
  let aliceJid: string;
  let bobJid: string;

  beforeAll(async () => {
    // Connect sequentially (like whatsapp-rust e2e tests) — Client A fully connects
    // before Client B starts, ensuring mock server state is clean.
    alice = await createTestClient("alice");
    alice.client.run();
    console.log("  Waiting for Alice to pair...");
    await waitForEvent(alice.events, "pair_success", 20000);
    console.log("  Alice paired! Waiting for connected...");
    await waitForEvent(alice.events, "connected", 45000);
    console.log("  Alice fully connected!");

    bob = await createTestClient("bob");
    bob.client.run();
    console.log("  Waiting for Bob to pair...");
    await waitForEvent(bob.events, "pair_success", 20000);
    console.log("  Bob paired! Waiting for connected...");
    await waitForEvent(bob.events, "connected", 45000);
    console.log("  Bob fully connected!");

    // getJid() returns the AD JID with device (e.g. "559980000014:33@s.whatsapp.net").
    // For addressing, we need the non-AD JID (without device), like whatsapp-rust's to_non_ad().
    const rawAliceJid = (await alice.client.getJid())!;
    const rawBobJid = (await bob.client.getJid())!;
    aliceJid = rawAliceJid.replace(/:[\d]+@/, "@");
    bobJid = rawBobJid.replace(/:[\d]+@/, "@");
    console.log(`  Alice JID: ${aliceJid} (raw: ${rawAliceJid})`);
    console.log(`  Bob JID: ${bobJid} (raw: ${rawBobJid})`);

    expect(aliceJid).toBeDefined();
    expect(bobJid).toBeDefined();
  }, 90000);

  afterAll(async () => {
    if (alice?.client) {
      await alice.client.disconnect();
      alice.client.free();
    }
    if (bob?.client) {
      await bob.client.disconnect();
      bob.client.free();
    }
  });

  test(
    "Alice sends a text message to Bob and Bob receives it",
    async () => {
      const text = `Hello Bob! ${Date.now()}`;
      const bobEventsBefore = bob.events.length;

      // Alice sends to Bob
      const msgId = await alice.client.sendMessage(bobJid, {
        conversation: text,
      });
      expect(msgId).toBeTruthy();
      console.log(`  Alice sent message (id=${msgId}) to Bob`);

      // Wait for Bob to receive Alice's message
      const received = await waitForEventMatching(
        bob.events,
        (e) => {
          if (!isMessageEvent(e)) return false;
          const data = e.data as MessageEventData;
          return (
            data.message?.conversation === text &&
            !getIsFromMe(data)
          );
        },
        15000,
        bobEventsBefore
      );

      const data = (received as { data: MessageEventData }).data;
      expect(data.message.conversation).toBe(text);
      expect(getIsFromMe(data)).toBe(false);
      console.log(
        `  Bob received: "${data.message.conversation}"`
      );
    },
    30000
  );

  test(
    "Bob sends a text message to Alice and Alice receives it",
    async () => {
      const text = `Hey Alice! ${Date.now()}`;
      const aliceEventsBefore = alice.events.length;

      // Bob sends to Alice
      const msgId = await bob.client.sendMessage(aliceJid, {
        conversation: text,
      });
      expect(msgId).toBeTruthy();
      console.log(`  Bob sent message (id=${msgId}) to Alice`);

      // Wait for Alice to receive Bob's message
      const received = await waitForEventMatching(
        alice.events,
        (e) => {
          if (!isMessageEvent(e)) return false;
          const data = e.data as MessageEventData;
          return (
            data.message?.conversation === text &&
            !getIsFromMe(data)
          );
        },
        15000,
        aliceEventsBefore
      );

      const data = (received as { data: MessageEventData }).data;
      expect(data.message.conversation).toBe(text);
      expect(getIsFromMe(data)).toBe(false);
      console.log(
        `  Alice received: "${data.message.conversation}"`
      );
    },
    30000
  );

  test(
    "Both clients exchange multiple messages in sequence",
    async () => {
      const messages = [
        { from: alice, to: bob, fromJid: aliceJid, toJid: bobJid, text: `Msg1 A→B ${Date.now()}` },
        { from: bob, to: alice, fromJid: bobJid, toJid: aliceJid, text: `Msg2 B→A ${Date.now()}` },
        { from: alice, to: bob, fromJid: aliceJid, toJid: bobJid, text: `Msg3 A→B ${Date.now()}` },
        { from: bob, to: alice, fromJid: bobJid, toJid: aliceJid, text: `Msg4 B→A ${Date.now()}` },
      ];

      for (const msg of messages) {
        const receiverEventsBefore = msg.to.events.length;

        const msgId = await msg.from.client.sendMessage(msg.toJid, {
          conversation: msg.text,
        });
        expect(msgId).toBeTruthy();
        console.log(`  ${msg.from.name} → ${msg.to.name}: "${msg.text}" (id=${msgId})`);

        // Wait for receiver to get the message
        const received = await waitForEventMatching(
          msg.to.events,
          (e) => {
            if (!isMessageEvent(e)) return false;
            const data = e.data as MessageEventData;
            return (
              data.message?.conversation === msg.text &&
              !getIsFromMe(data)
            );
          },
          15000,
          receiverEventsBefore
        );

        const data = (received as { data: MessageEventData }).data;
        expect(data.message.conversation).toBe(msg.text);
        expect(getIsFromMe(data)).toBe(false);
        console.log(`  ${msg.to.name} received: "${data.message.conversation}"`);
      }
    },
    60000
  );

  test(
    "messages arrive quickly (under 2 seconds)",
    async () => {
      const text = `Latency test ${Date.now()}`;
      const bobEventsBefore = bob.events.length;
      const start = Date.now();

      await alice.client.sendMessage(bobJid, { conversation: text });

      const received = await waitForEventMatching(
        bob.events,
        (e) => {
          if (!isMessageEvent(e)) return false;
          const data = e.data as MessageEventData;
          return data.message?.conversation === text && !getIsFromMe(data);
        },
        5000,
        bobEventsBefore
      );

      const elapsed = Date.now() - start;
      console.log(`  Message delivered in ${elapsed}ms`);
      expect(elapsed).toBeLessThan(2000);

      const data = (received as { data: MessageEventData }).data;
      expect(data.message.conversation).toBe(text);
    },
    10000
  );
});
