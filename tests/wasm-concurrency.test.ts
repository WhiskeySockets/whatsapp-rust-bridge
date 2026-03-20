/**
 * Tests that concurrent WASM operations don't deadlock or busy-spin.
 *
 * The root issue: in single-threaded WASM, multiple spawned tasks contending
 * on async_lock::Mutex would busy-spin at 100% CPU because the executor
 * polled them in a tight loop without yielding to the JS event loop.
 * The fix adds yield_to_js() in WasmRuntime::spawn.
 *
 * These tests verify:
 * 1. Multiple concurrent encodeProto calls complete without hanging
 * 2. Multiple concurrent decodeProto calls complete without hanging
 * 3. Mixed encode+decode operations interleave correctly
 * 4. Operations complete within a reasonable time (not spinning)
 *
 * Run: bun test tests/wasm-concurrency.test.ts
 */

import { describe, test, expect, beforeAll } from "bun:test";
import { initWasmEngine, encodeProto, decodeProto } from "../dist/index.js";

beforeAll(() => {
  initWasmEngine();
});

describe("WASM concurrency (no deadlock/busy-spin)", () => {
  test(
    "concurrent encodeProto calls complete within timeout",
    async () => {
      const messages = Array.from({ length: 50 }, (_, i) => ({
        extendedTextMessage: { text: `concurrent msg ${i}` },
      }));

      const start = Date.now();
      const results = await Promise.all(
        messages.map((msg) => {
          return new Promise<Uint8Array>((resolve) => {
            // Use setTimeout to simulate concurrent spawning
            setTimeout(() => resolve(encodeProto("Message", msg)), 0);
          });
        })
      );
      const elapsed = Date.now() - start;

      expect(results.length).toBe(50);
      for (const bytes of results) {
        expect(bytes).toBeInstanceOf(Uint8Array);
        expect(bytes.length).toBeGreaterThan(0);
      }

      // Should complete quickly — not spin for minutes
      console.log(`  50 concurrent encodes completed in ${elapsed}ms`);
      expect(elapsed).toBeLessThan(5000);
    },
    10000
  );

  test(
    "concurrent decodeProto calls complete within timeout",
    async () => {
      // First encode 50 messages
      const encoded = Array.from({ length: 50 }, (_, i) =>
        encodeProto("Message", {
          extendedTextMessage: { text: `decode test ${i}` },
        })
      );

      const start = Date.now();
      const results = await Promise.all(
        encoded.map((bytes) => {
          return new Promise<unknown>((resolve) => {
            setTimeout(() => resolve(decodeProto("Message", bytes)), 0);
          });
        })
      );
      const elapsed = Date.now() - start;

      expect(results.length).toBe(50);
      for (let i = 0; i < results.length; i++) {
        const decoded = results[i] as Record<string, any>;
        expect(decoded.extendedTextMessage.text).toBe(`decode test ${i}`);
      }

      console.log(`  50 concurrent decodes completed in ${elapsed}ms`);
      expect(elapsed).toBeLessThan(5000);
    },
    10000
  );

  test(
    "interleaved encode and decode operations complete",
    async () => {
      const start = Date.now();
      const ops: Promise<unknown>[] = [];

      for (let i = 0; i < 30; i++) {
        // Encode
        ops.push(
          new Promise<unknown>((resolve) => {
            setTimeout(() => {
              const bytes = encodeProto("Message", {
                conversation: `interleave ${i}`,
              });
              resolve(bytes);
            }, 0);
          })
        );

        // Decode a previously encoded message
        const preEncoded = encodeProto("Message", {
          conversation: `pre-encoded ${i}`,
        });
        ops.push(
          new Promise<unknown>((resolve) => {
            setTimeout(() => {
              const decoded = decodeProto("Message", preEncoded);
              resolve(decoded);
            }, 0);
          })
        );
      }

      const results = await Promise.all(ops);
      const elapsed = Date.now() - start;

      expect(results.length).toBe(60);
      console.log(`  60 interleaved encode/decode ops completed in ${elapsed}ms`);
      expect(elapsed).toBeLessThan(5000);
    },
    10000
  );

  test(
    "rapid sequential operations don't starve the event loop",
    async () => {
      // This simulates what happens during offline sync: many messages
      // arrive rapidly and each needs encode/decode

      const start = Date.now();
      let eventLoopRan = false;

      // Schedule a check on the event loop
      const eventLoopPromise = new Promise<void>((resolve) => {
        setTimeout(() => {
          eventLoopRan = true;
          resolve();
        }, 10);
      });

      // Run 100 synchronous encode operations
      for (let i = 0; i < 100; i++) {
        encodeProto("Message", {
          extendedTextMessage: { text: `rapid ${i}` },
        });
      }

      // The event loop callback should still fire
      await eventLoopPromise;
      const elapsed = Date.now() - start;

      expect(eventLoopRan).toBe(true);
      console.log(`  100 rapid encodes + event loop check: ${elapsed}ms`);
      expect(elapsed).toBeLessThan(5000);
    },
    10000
  );

  test(
    "encode roundtrip under concurrent load produces correct results",
    async () => {
      // Verify data integrity under concurrent load
      const messages = Array.from({ length: 20 }, (_, i) => ({
        extendedTextMessage: {
          text: `integrity check ${i} - ${Math.random().toString(36)}`,
          contextInfo: {
            stanzaId: `stanza-${i}`,
            participant: `55119${String(i).padStart(5, "0")}@s.whatsapp.net`,
          },
        },
      }));

      const results = await Promise.all(
        messages.map(async (msg, i) => {
          // Stagger starts slightly
          await new Promise((r) => setTimeout(r, i % 5));
          const bytes = encodeProto("Message", msg);
          const decoded = decodeProto("Message", bytes) as Record<string, any>;
          return {
            original: msg.extendedTextMessage.text,
            decoded: decoded.extendedTextMessage?.text,
            stanzaId: decoded.extendedTextMessage?.contextInfo?.stanzaId,
          };
        })
      );

      for (let i = 0; i < results.length; i++) {
        expect(results[i].decoded).toBe(results[i].original);
        expect(results[i].stanzaId).toBe(`stanza-${i}`);
      }
    },
    10000
  );
});
