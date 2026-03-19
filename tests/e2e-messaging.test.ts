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
import WebSocket from "ws";
import {
  initWasmEngine,
  createWhatsAppClient,
} from "../dist/index.js";
import type {
  JsTransportCallbacks,
  JsTransportHandle,
  JsHttpClientConfig,
  Event,
  WasmWhatsAppClient,
} from "../types/index.js";

const MOCK_SERVER_URL =
  process.env.MOCK_SERVER_URL ?? "wss://127.0.0.1:8080/ws/chat";

process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";

beforeAll(() => {
  initWasmEngine();
});

// ---------------------------------------------------------------------------
// Transport: properly handles reconnection without races
// ---------------------------------------------------------------------------

function createTransport(label: string): JsTransportCallbacks {
  // Track WebSocket per-connection. The key insight: disconnect() must close
  // the WS that was active BEFORE the current connect() call, not the one
  // created BY the current connect() call. We use a Map keyed by generation
  // to ensure disconnect() only affects the right WebSocket.
  let activeWs: WebSocket | null = null;
  let disconnectTarget: WebSocket | null = null;

  return {
    connect(handle: JsTransportHandle) {
      // The WS to disconnect is whatever was active before this connect()
      // The Rust client calls: disconnect(old) → create_transport() → connect(new)
      // But sometimes create_transport() is called BEFORE disconnect() finishes,
      // so we capture the "old" WS here for disconnect() to close later.
      disconnectTarget = activeWs;
      if (activeWs) {
        activeWs.removeAllListeners();
      }

      console.log(`  [${label}] ws connecting...`);
      const ws = new WebSocket(MOCK_SERVER_URL, { rejectUnauthorized: false });
      ws.binaryType = "arraybuffer";
      activeWs = ws;

      return new Promise<void>((resolve, reject) => {
        ws.on("open", () => {
          if (activeWs !== ws) return; // Superseded by newer connect()
          console.log(`  [${label}] ws connected`);
          handle.onConnected();
          resolve();
        });
        ws.on("message", (data: ArrayBuffer | Buffer) => {
          if (activeWs !== ws) return;
          const bytes =
            data instanceof ArrayBuffer
              ? new Uint8Array(data)
              : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
          handle.onData(bytes);
        });
        ws.on("close", () => {
          if (activeWs !== ws) return;
          console.log(`  [${label}] ws disconnected`);
          handle.onDisconnected();
        });
        ws.on("error", (err) => {
          if (activeWs !== ws) return;
          console.error(`  [${label}] ws error: ${err.message}`);
          reject(err);
        });
      });
    },
    send(data: Uint8Array) {
      if (activeWs?.readyState === WebSocket.OPEN) {
        activeWs.send(data);
      }
    },
    disconnect() {
      // Close the disconnect target (the OLD WebSocket), NOT activeWs
      // (which might already be a new connection from a concurrent connect() call)
      const toClose = disconnectTarget ?? activeWs;
      if (toClose) {
        toClose.removeAllListeners();
        toClose.close();
      }
      if (toClose === activeWs) {
        activeWs = null;
      }
      disconnectTarget = null;
    },
  } as unknown as JsTransportCallbacks;
}

function createHttp(): JsHttpClientConfig {
  return {
    async execute(
      url: string,
      method: string,
      headers: Record<string, string>,
      body: Uint8Array | null
    ) {
      try {
        const res = await fetch(url, {
          method,
          headers,
          body: body ?? undefined,
        });
        return {
          statusCode: res.status,
          body: new Uint8Array(await res.arrayBuffer()),
        };
      } catch {
        return { statusCode: 0, body: new Uint8Array(0) };
      }
    },
  } as unknown as JsHttpClientConfig;
}

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

function waitForEvent(
  tc: TestClient,
  type: string,
  timeoutMs = 30000
): Promise<Event> {
  return new Promise((resolve, reject) => {
    const existing = tc.events.find((e) => e.type === type);
    if (existing) { resolve(existing); return; }

    const deadline = Date.now() + timeoutMs;
    const interval = setInterval(() => {
      const found = tc.events.find((e) => e.type === type);
      if (found) {
        clearInterval(interval);
        resolve(found);
      } else if (Date.now() > deadline) {
        clearInterval(interval);
        reject(new Error(
          `[${tc.name}] Timed out waiting for '${type}'. Got: ${tc.events.map((e) => e.type).join(", ")}`
        ));
      }
    }, 100);
  });
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
        waitForEvent(alice, "pair_success", 20000),
        waitForEvent(bob, "pair_success", 20000),
      ]);
      console.log("  Both paired!");

      // Wait for full login (offline_sync_completed proves login handshake worked)
      console.log("  Waiting for login...");
      const [aliceLogin, bobLogin] = await Promise.allSettled([
        waitForEvent(alice, "offline_sync_completed", 45000),
        waitForEvent(bob, "offline_sync_completed", 45000),
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
