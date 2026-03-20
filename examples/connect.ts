/**
 * Example: Connect a WhatsApp client to the mock server.
 *
 * Prerequisites:
 *   - Mock server running on wss://127.0.0.1:8080/ws/chat
 *   - Bridge built: bun run build:dev
 *
 * Run: bun run example
 */

import { initWasmEngine, createWhatsAppClient } from "../dist/index.js";
import { createTransport, createHttp } from "../tests/helpers.js";

process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";

async function main() {
  initWasmEngine();

  console.log("Creating WhatsApp client...");

  const client = await createWhatsAppClient(
    createTransport("client"),
    createHttp(),
    (event) => {
      console.log(`[event] ${event.type}`);

      if (event.type === "qr") {
        console.log(
          "\n📱 Scan QR code or wait for auto-pair (mock server)...\n",
        );
      }

      if (event.type === "pair_success") {
        console.log("\n✅ Paired successfully!\n");
      }

      if (event.type === "offline_sync_completed") {
        console.log("\n🟢 Fully connected and synced!\n");
        client.getJid().then((jid) => console.log(`   JID: ${jid}`));
        client.getLid().then((lid) => console.log(`   LID: ${lid}`));
      }

      if (event.type === "message") {
        console.log(`Message: ${JSON.stringify(event.data.message)}`);
      }
    },
  );

  console.log("Starting client...");
  client.run();

  // Keep alive until Ctrl+C
  process.on("SIGINT", async () => {
    console.log("\nDisconnecting...");
    await client.disconnect();
    client.free();
    process.exit(0);
  });

  await new Promise(() => {});
}

main().catch(console.error);
