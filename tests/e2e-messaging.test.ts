/**
 * E2E test: Two WASM clients connect, pair, and login.
 *
 * Prerequisites:
 *   - Mock server running on wss://127.0.0.1:8080/ws/chat
 *   - Bridge built: bun run build:dev
 *
 * Run: NODE_TLS_REJECT_UNAUTHORIZED=0 bun test tests/e2e-messaging.test.ts
 */

import { describe, test, expect, beforeAll } from "bun:test";
import {
  initWasmEngine,
  createWhatsAppClient,
} from "../dist/index.js";
import type {
  Event,
  WasmWhatsAppClient,
} from "../types/index.js";
import { createTransport, createHttp, waitForEvent } from "./helpers.js";

process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";

beforeAll(() => {
  initWasmEngine();
});

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

interface TestClient {
  client: WasmWhatsAppClient;
  events: Event[];
  name: string;
}

async function createTestClient(name: string): Promise<TestClient> {
  const events: Event[] = [];
  const client = await createWhatsAppClient(
    createTransport(name) as any,
    createHttp() as any,
    ((event: Event) => {
      console.log(`  [${name}] event: ${event.type}`);
      events.push(event);
    }) as any
  );
  return { client, events, name };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("Two-client E2E", () => {
  test(
    "both clients connect, pair, and login",
    async () => {
      const alice = await createTestClient("alice");
      const bob = await createTestClient("bob");

      alice.client.run();
      bob.client.run();

      // Wait for pairing
      console.log("  Waiting for pairing...");
      await Promise.all([
        waitForEvent(alice.events, "pair_success", 20000),
        waitForEvent(bob.events, "pair_success", 20000),
      ]);
      console.log("  Both paired!");

      // Wait for full login (offline_sync_completed proves login handshake worked)
      console.log("  Waiting for login...");
      const [aliceLogin, bobLogin] = await Promise.allSettled([
        waitForEvent(alice.events, "offline_sync_completed", 45000),
        waitForEvent(bob.events, "offline_sync_completed", 45000),
      ]);

      const aliceOk = aliceLogin.status === "fulfilled";
      const bobOk = bobLogin.status === "fulfilled";
      console.log(`  Alice logged in: ${aliceOk}, Bob logged in: ${bobOk}`);

      // Both should be logged in
      expect(aliceOk).toBe(true);
      expect(bobOk).toBe(true);
      expect(alice.client.isLoggedIn()).toBe(true);
      expect(bob.client.isLoggedIn()).toBe(true);

      const aliceJid = await alice.client.getJid();
      const bobJid = await bob.client.getJid();
      console.log(`  Alice: ${aliceJid}, Bob: ${bobJid}`);
      expect(aliceJid).toBeDefined();
      expect(bobJid).toBeDefined();

      // Cleanup
      await alice.client.disconnect();
      await bob.client.disconnect();
      alice.client.free();
      bob.client.free();
    },
    90000
  );
});
