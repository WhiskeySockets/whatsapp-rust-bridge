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
  Jid,
} from "../pkg/whatsapp_rust_bridge.js";
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
  info: MessageInfo;
};

function isIncomingMessage(
  e: WhatsAppEvent,
  text: string
): e is WhatsAppEvent & { type: "message"; data: MessageEventData } {
  if (e.type !== "message") return false;
  const data = e.data as MessageEventData;
  return data.message?.conversation === text && !data.info.source.is_from_me;
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
    await waitForEvent(alice.events, "pair_success", 20000);
    await waitForEvent(alice.events, "connected", 45000);
    console.log("  Alice connected!");

    bob = await createTestClient("bob");
    bob.client.run();
    await waitForEvent(bob.events, "pair_success", 20000);
    await waitForEvent(bob.events, "connected", 45000);
    console.log("  Bob connected!");

    // getJid() returns non-AD JID string for addressing (e.g. "559980000014@s.whatsapp.net")
    aliceJid = (await alice.client.getJid())!;
    bobJid = (await bob.client.getJid())!;
    console.log(`  Alice: ${aliceJid}, Bob: ${bobJid}`);

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
      const before = bob.events.length;

      const msgId = await alice.client.sendMessage(bobJid, { conversation: text });
      expect(msgId).toBeTruthy();
      console.log(`  Alice → Bob: "${text}" (id=${msgId})`);

      const event = await waitForEventMatching(
        bob.events,
        (e) => isIncomingMessage(e, text),
        15000,
        before
      );

      const data = (event as { data: MessageEventData }).data;
      expect(data.message.conversation).toBe(text);
      expect(data.info.source.is_from_me).toBe(false);
      expect(data.info.source.sender.user).toBeTruthy();
      expect(data.info.source.sender.server).toBe("s.whatsapp.net");
      console.log(
        `  Bob received: "${data.message.conversation}" from ${data.info.source.sender.user}@${data.info.source.sender.server}`
      );
    },
    30000
  );

  test(
    "Bob sends a text message to Alice and Alice receives it",
    async () => {
      const text = `Hey Alice! ${Date.now()}`;
      const before = alice.events.length;

      const msgId = await bob.client.sendMessage(aliceJid, { conversation: text });
      expect(msgId).toBeTruthy();
      console.log(`  Bob → Alice: "${text}" (id=${msgId})`);

      const event = await waitForEventMatching(
        alice.events,
        (e) => isIncomingMessage(e, text),
        15000,
        before
      );

      const data = (event as { data: MessageEventData }).data;
      expect(data.message.conversation).toBe(text);
      expect(data.info.source.is_from_me).toBe(false);
      console.log(
        `  Alice received: "${data.message.conversation}" from ${data.info.source.sender.user}@${data.info.source.sender.server}`
      );
    },
    30000
  );

  test(
    "Both clients exchange multiple messages in sequence",
    async () => {
      const exchanges = [
        { from: alice, to: bob, toJid: bobJid, text: `Msg1 A→B ${Date.now()}` },
        { from: bob, to: alice, toJid: aliceJid, text: `Msg2 B→A ${Date.now()}` },
        { from: alice, to: bob, toJid: bobJid, text: `Msg3 A→B ${Date.now()}` },
        { from: bob, to: alice, toJid: aliceJid, text: `Msg4 B→A ${Date.now()}` },
      ];

      for (const { from, to, toJid, text } of exchanges) {
        const before = to.events.length;

        const msgId = await from.client.sendMessage(toJid, { conversation: text });
        expect(msgId).toBeTruthy();
        console.log(`  ${from.name} → ${to.name}: "${text}"`);

        const event = await waitForEventMatching(
          to.events,
          (e) => isIncomingMessage(e, text),
          15000,
          before
        );

        const data = (event as { data: MessageEventData }).data;
        expect(data.message.conversation).toBe(text);
        expect(data.info.source.is_from_me).toBe(false);
        console.log(`  ${to.name} received: "${data.message.conversation}"`);
      }
    },
    60000
  );

  test(
    "messages arrive quickly (under 2 seconds)",
    async () => {
      const text = `Latency test ${Date.now()}`;
      const before = bob.events.length;
      const start = Date.now();

      await alice.client.sendMessage(bobJid, { conversation: text });

      await waitForEventMatching(
        bob.events,
        (e) => isIncomingMessage(e, text),
        5000,
        before
      );

      const elapsed = Date.now() - start;
      console.log(`  Message delivered in ${elapsed}ms`);
      expect(elapsed).toBeLessThan(2000);
    },
    10000
  );
});
