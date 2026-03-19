/**
 * E2E test: WASM client connects to mock server, pairs successfully.
 *
 * Run: NODE_TLS_REJECT_UNAUTHORIZED=0 bun test tests/e2e-connect.test.ts
 */

import { describe, test, expect, beforeAll } from "bun:test";
import {
  initWasmEngine,
  createWhatsAppClient,
  type WhatsAppEvent,
} from "../dist/index.js";
import { createTransport, createHttp, waitForEvent } from "./helpers.js";

process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";

beforeAll(() => {
  initWasmEngine();
});

describe("WASM Client E2E", () => {
  test("creates client in disconnected state", async () => {
    const client = await createWhatsAppClient(
      { connect() {}, send() {}, disconnect() {} },
      createHttp()
    );
    expect(client.isConnected()).toBe(false);
    expect(client.isLoggedIn()).toBe(false);
    client.free();
  });

  test("connects and pairs with mock server", async () => {
    const events: WhatsAppEvent[] = [];

    const client = await createWhatsAppClient(
      createTransport(),
      createHttp(),
      (event) => {
        console.log(`  [event] ${event.type}`);
        events.push(event);
      }
    );

    client.run();

    await waitForEvent(events, "pair_success", 20000);

    console.log("  Events:", events.map(e => e.type));
    expect(events.some(e => e.type === "qr")).toBe(true);
    expect(events.some(e => e.type === "pair_success")).toBe(true);

    await client.disconnect();
    client.free();
  }, 25000);
});
