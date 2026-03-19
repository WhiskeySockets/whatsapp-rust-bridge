/**
 * Shared test helpers for E2E tests.
 */

import WebSocket from "ws";
import type {
  JsTransportCallbacks,
  JsTransportHandle,
  JsHttpClientConfig,
  Event,
} from "../types/index.js";

const MOCK_SERVER_URL =
  process.env.MOCK_SERVER_URL ?? "wss://127.0.0.1:8080/ws/chat";

/**
 * Create a WebSocket transport with proper reconnection handling.
 *
 * Uses a `disconnectTarget` to ensure disconnect() closes the correct
 * (old) WebSocket even when a new connect() has already started.
 */
export function createTransport(label?: string): JsTransportCallbacks {
  let activeWs: WebSocket | null = null;
  let disconnectTarget: WebSocket | null = null;

  return {
    connect(handle: JsTransportHandle) {
      // The WS to disconnect is whatever was active before this connect()
      // The Rust client calls: disconnect(old) -> create_transport() -> connect(new)
      // But sometimes create_transport() is called BEFORE disconnect() finishes,
      // so we capture the "old" WS here for disconnect() to close later.
      disconnectTarget = activeWs;
      if (activeWs) {
        activeWs.removeAllListeners();
      }

      if (label) console.log(`  [${label}] ws connecting...`);
      const ws = new WebSocket(MOCK_SERVER_URL, { rejectUnauthorized: false });
      ws.binaryType = "arraybuffer";
      activeWs = ws;

      return new Promise<void>((resolve, reject) => {
        ws.on("open", () => {
          if (activeWs !== ws) return; // Superseded by newer connect()
          if (label) console.log(`  [${label}] ws connected`);
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
          if (label) console.log(`  [${label}] ws disconnected`);
          handle.onDisconnected();
        });
        ws.on("error", (err) => {
          if (activeWs !== ws) return;
          if (label) console.error(`  [${label}] ws error: ${err.message}`);
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
  } satisfies JsTransportCallbacks;
}

/**
 * Create an HTTP client adapter backed by fetch().
 */
export function createHttp(): JsHttpClientConfig {
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
  } satisfies JsHttpClientConfig;
}

/**
 * Wait for a specific event type to appear in an event array.
 */
export function waitForEvent(
  events: Event[],
  type: string,
  timeoutMs = 30000
): Promise<Event> {
  return new Promise((resolve, reject) => {
    const existing = events.find((e) => e.type === type);
    if (existing) {
      resolve(existing);
      return;
    }

    const deadline = Date.now() + timeoutMs;
    const interval = setInterval(() => {
      const found = events.find((e) => e.type === type);
      if (found) {
        clearInterval(interval);
        resolve(found);
      } else if (Date.now() > deadline) {
        clearInterval(interval);
        reject(
          new Error(
            `Timed out waiting for '${type}'. Got: ${events.map((e) => e.type).join(", ")}`
          )
        );
      }
    }, 100);
  });
}
