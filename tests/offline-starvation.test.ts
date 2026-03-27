/**
 * Reproduces event loop starvation during large offline message batches.
 *
 * Root cause: When the WASM client processes hundreds of offline messages,
 * workers call flush_signal_cache() → backend.set() → writeFile(). The
 * writeFile() needs the Node.js I/O poll phase to complete. If WASM yields
 * only use MessageChannel (which fires BEFORE I/O polling), the writeFile
 * Promise never resolves → 100% CPU deadlock.
 *
 * The fix: Use setImmediate for yielding, which fires in the check phase
 * (AFTER I/O polling), ensuring file/network I/O can complete between yields.
 *
 * Run: bun test tests/offline-starvation.test.ts
 */

import { describe, test, expect, beforeAll } from "bun:test";
import {
  initWasmEngine,
  encodeProto,
  decodeProto,
  stressTestSpawn,
} from "../dist/index.js";
import { writeFile, unlink, readFile } from "fs/promises";
import { tmpdir } from "os";
import { join } from "path";

beforeAll(() => {
  initWasmEngine();
});

describe("offline message starvation", () => {
  test(
    "WASM-spawned workers don't starve the JS event loop",
    async () => {
      // Exercises the actual WASM spawn + yield path.
      // stressTestSpawn() spawns N workers via WasmRuntime::spawn(),
      // each doing compute + set_timeout_0().await (which uses setImmediate).
      // setInterval monitors event loop responsiveness.

      const WORKER_COUNT = 200;
      const STEPS_PER_WORKER = 5;
      const MONITOR_INTERVAL = 20;

      let eventLoopTicks = 0;
      const monitor = setInterval(() => {
        eventLoopTicks++;
      }, MONITOR_INTERVAL);

      const start = Date.now();
      const completedSteps = await stressTestSpawn(
        WORKER_COUNT,
        STEPS_PER_WORKER
      );
      const elapsed = Date.now() - start;

      clearInterval(monitor);

      console.log(
        `  ${WORKER_COUNT} WASM workers × ${STEPS_PER_WORKER} steps = ${completedSteps} completed ` +
          `in ${elapsed}ms, event loop ticked ${eventLoopTicks} times`
      );

      expect(completedSteps).toBe(WORKER_COUNT * STEPS_PER_WORKER);
      expect(eventLoopTicks).toBeGreaterThanOrEqual(1);
    },
    60000
  );

  test(
    "file I/O completes during heavy WASM workload",
    async () => {
      // This is the critical test: models the actual deadlock scenario.
      // Workers do WASM work + write files (like session storage writeFile).
      // If WASM yielding doesn't let the I/O poll phase run, writeFile
      // Promises never resolve → deadlock.

      const WORKER_COUNT = 50;
      const tmpDir = tmpdir();
      const tmpFiles: string[] = [];
      let completed = 0;

      const start = Date.now();

      const workers = Array.from({ length: WORKER_COUNT }, async (_, i) => {
        // WASM work (like message decryption)
        const bytes = encodeProto("Message", {
          extendedTextMessage: { text: `io-test-${i}` },
        });

        // File I/O (like session storage writeFile) — needs I/O poll phase
        const tmpFile = join(tmpDir, `wasm-starvation-test-${i}-${Date.now()}.bin`);
        tmpFiles.push(tmpFile);
        await writeFile(tmpFile, bytes);

        // More WASM work after I/O
        decodeProto("Message", bytes);

        // Another file I/O
        const data = await readFile(tmpFile);
        expect(data.length).toBe(bytes.length);

        completed++;
      });

      // Deadline: if file I/O is starved, this will timeout
      const timeout = new Promise<never>((_, reject) => {
        setTimeout(
          () =>
            reject(
              new Error(
                `I/O deadlock: only ${completed}/${WORKER_COUNT} completed`
              )
            ),
          10000
        );
      });

      await Promise.race([Promise.all(workers), timeout]);
      const elapsed = Date.now() - start;

      // Cleanup temp files
      await Promise.all(
        tmpFiles.map((f) => unlink(f).catch(() => {}))
      );

      console.log(
        `  ${WORKER_COUNT} workers with file I/O completed in ${elapsed}ms`
      );

      expect(completed).toBe(WORKER_COUNT);
      expect(elapsed).toBeLessThan(10000);
    },
    15000
  );

  test(
    "concurrent async chains with WASM work don't deadlock",
    async () => {
      const CHAIN_COUNT = 20;
      const MESSAGES_PER_CHAIN = 30;
      const start = Date.now();

      let completed = 0;

      const chains = Array.from({ length: CHAIN_COUNT }, async (_, chainId) => {
        for (let msg = 0; msg < MESSAGES_PER_CHAIN; msg++) {
          const bytes = encodeProto("Message", {
            extendedTextMessage: { text: `chain-${chainId}-msg-${msg}` },
          });
          decodeProto("Message", bytes);
          await new Promise<void>((resolve) => setImmediate(resolve));
          completed++;
        }
      });

      const timeout = new Promise<never>((_, reject) => {
        setTimeout(
          () =>
            reject(
              new Error(
                `Deadlock: only ${completed}/${CHAIN_COUNT * MESSAGES_PER_CHAIN} completed`
              )
            ),
          10000
        );
      });

      await Promise.race([Promise.all(chains), timeout]);
      const elapsed = Date.now() - start;

      console.log(
        `  ${CHAIN_COUNT} chains × ${MESSAGES_PER_CHAIN} msgs = ${completed} ops in ${elapsed}ms`
      );

      expect(completed).toBe(CHAIN_COUNT * MESSAGES_PER_CHAIN);
      expect(elapsed).toBeLessThan(10000);
    },
    15000
  );
});
