/**
 * E2E test: WASM client connects to mock server, pairs successfully.
 *
 * Run: NODE_TLS_REJECT_UNAUTHORIZED=0 bun test tests/e2e-connect.test.ts
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
} from "../types/index.js";

const MOCK_SERVER_URL =
  process.env.MOCK_SERVER_URL ?? "wss://127.0.0.1:8080/ws/chat";

process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";

beforeAll(() => {
  initWasmEngine();
});

function createHttp(): JsHttpClientConfig {
  return {
    async execute(url: string, method: string, headers: Record<string, string>, body: Uint8Array | null) {
      try {
        const res = await fetch(url, { method, headers, body: body ?? undefined });
        return { statusCode: res.status, body: new Uint8Array(await res.arrayBuffer()) };
      } catch { return { statusCode: 0, body: new Uint8Array(0) }; }
    },
  } as unknown as JsHttpClientConfig;
}

function createTransport(): JsTransportCallbacks {
  const state = { ws: null as WebSocket | null };

  return {
    connect(handle: JsTransportHandle) {
      if (state.ws) state.ws.removeAllListeners();

      const ws = new WebSocket(MOCK_SERVER_URL, { rejectUnauthorized: false });
      ws.binaryType = "arraybuffer";
      state.ws = ws;

      return new Promise<void>((resolve, reject) => {
        ws.on("open", () => { handle.onConnected(); resolve(); });
        ws.on("message", (data: ArrayBuffer | Buffer) => {
          const bytes = data instanceof ArrayBuffer
            ? new Uint8Array(data)
            : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
          handle.onData(bytes);
        });
        ws.on("close", () => handle.onDisconnected());
        ws.on("error", (err) => reject(err));
      });
    },
    send(data: Uint8Array) {
      if (state.ws?.readyState === WebSocket.OPEN) state.ws.send(data);
    },
    disconnect() {
      state.ws?.close();
      state.ws = null;
    },
  } as unknown as JsTransportCallbacks;
}

describe("WASM Client E2E", () => {
  test("creates client in disconnected state", async () => {
    const client = await createWhatsAppClient(
      { connect() {}, send() {}, disconnect() {} } as unknown as JsTransportCallbacks,
      createHttp() as any
    );
    expect(client.isConnected()).toBe(false);
    expect(client.isLoggedIn()).toBe(false);
    client.free();
  });

  test("connects and pairs with mock server", async () => {
    const events: Event[] = [];

    const client = await createWhatsAppClient(
      createTransport() as any,
      createHttp() as any,
      ((event: Event) => {
        console.log(`  [event] ${event.type}`);
        events.push(event);
      }) as any
    );

    client.run();

    // Wait for pair_success
    const deadline = Date.now() + 20000;
    while (Date.now() < deadline && !events.some(e => e.type === "pair_success")) {
      await new Promise(r => setTimeout(r, 100));
    }

    console.log("  Events:", events.map(e => e.type));
    expect(events.some(e => e.type === "qr")).toBe(true);
    expect(events.some(e => e.type === "pair_success")).toBe(true);

    await client.disconnect();
    client.free();
  }, 25000);
});
