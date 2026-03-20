/**
 * Proto encode/decode roundtrip tests.
 * Verifies that encodeProto → decodeProto produces correct output,
 * matching what prost would produce natively.
 *
 * Run: bun test tests/proto-roundtrip.test.ts
 */

import { describe, test, expect, beforeAll } from "bun:test";
import { initWasmEngine, encodeProto, decodeProto } from "../dist/index.js";

beforeAll(() => {
  initWasmEngine();
});

describe("encodeProto / decodeProto roundtrip", () => {
  test("simple conversation message", () => {
    const msg = { conversation: "Hello world" };
    const bytes = encodeProto("Message", msg);
    expect(bytes).toBeInstanceOf(Uint8Array);
    expect(bytes.length).toBeGreaterThan(0);

    const decoded = decodeProto("Message", bytes);
    expect(decoded.conversation).toBe("Hello world");
  });

  test("extendedTextMessage", () => {
    const msg = {
      extendedTextMessage: {
        text: "Hello with extended",
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.extendedTextMessage.text).toBe("Hello with extended");
  });

  test("extendedTextMessage with contextInfo", () => {
    const msg = {
      extendedTextMessage: {
        text: "pong test",
        contextInfo: {
          stanzaId: "ABC123",
          participant: "5511999@s.whatsapp.net",
        },
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.extendedTextMessage.text).toBe("pong test");
    expect(decoded.extendedTextMessage.contextInfo.stanzaId).toBe("ABC123");
    expect(decoded.extendedTextMessage.contextInfo.participant).toBe(
      "5511999@s.whatsapp.net"
    );
  });

  test("message with messageContextInfo and messageSecret (Buffer)", () => {
    const secret = new Uint8Array(32);
    crypto.getRandomValues(secret);

    const msg = {
      extendedTextMessage: { text: "test with secret" },
      messageContextInfo: {
        messageSecret: secret,
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);

    expect(decoded.extendedTextMessage.text).toBe("test with secret");
    expect(decoded.messageContextInfo).toBeDefined();
    expect(decoded.messageContextInfo.messageSecret).toBeInstanceOf(Uint8Array);
    expect(decoded.messageContextInfo.messageSecret.length).toBe(32);
    // Verify bytes match
    for (let i = 0; i < 32; i++) {
      expect(decoded.messageContextInfo.messageSecret[i]).toBe(secret[i]);
    }
  });

  test("message with messageSecret as Node Buffer", () => {
    const secret = Buffer.from(crypto.getRandomValues(new Uint8Array(32)));

    const msg = {
      extendedTextMessage: { text: "test buffer secret" },
      messageContextInfo: {
        messageSecret: secret,
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);

    expect(decoded.extendedTextMessage.text).toBe("test buffer secret");
    expect(decoded.messageContextInfo.messageSecret).toBeInstanceOf(Uint8Array);
    expect(decoded.messageContextInfo.messageSecret.length).toBe(32);
  });

  test("reactionMessage", () => {
    const msg = {
      reactionMessage: {
        key: {
          remoteJid: "5511999@s.whatsapp.net",
          fromMe: false,
          id: "MSG123",
        },
        text: "👍",
        senderTimestampMs: 1700000000000,
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.reactionMessage.text).toBe("👍");
    expect(decoded.reactionMessage.key.id).toBe("MSG123");
  });

  test("imageMessage with bytes fields", () => {
    const mediaKey = new Uint8Array(32);
    const fileSha256 = new Uint8Array(32);
    const fileEncSha256 = new Uint8Array(32);
    crypto.getRandomValues(mediaKey);
    crypto.getRandomValues(fileSha256);
    crypto.getRandomValues(fileEncSha256);

    const msg = {
      imageMessage: {
        url: "https://mmg.whatsapp.net/test",
        mimetype: "image/jpeg",
        caption: "test image",
        fileSha256: fileSha256,
        fileLength: 12345,
        mediaKey: mediaKey,
        fileEncSha256: fileEncSha256,
        directPath: "/mms/image/test",
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);

    expect(decoded.imageMessage.url).toBe("https://mmg.whatsapp.net/test");
    expect(decoded.imageMessage.caption).toBe("test image");
    expect(decoded.imageMessage.mimetype).toBe("image/jpeg");
    expect(decoded.imageMessage.mediaKey).toBeInstanceOf(Uint8Array);
    expect(decoded.imageMessage.mediaKey.length).toBe(32);
    expect(decoded.imageMessage.fileSha256).toBeInstanceOf(Uint8Array);
    expect(decoded.imageMessage.fileSha256.length).toBe(32);
  });

  test("protocolMessage REVOKE", () => {
    const msg = {
      protocolMessage: {
        key: {
          remoteJid: "5511999@s.whatsapp.net",
          fromMe: true,
          id: "MSG_TO_DELETE",
        },
        type: 0, // REVOKE
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.protocolMessage.key.id).toBe("MSG_TO_DELETE");
  });

  test("protocolMessage MESSAGE_EDIT", () => {
    const msg = {
      protocolMessage: {
        key: {
          remoteJid: "5511999@s.whatsapp.net",
          fromMe: true,
          id: "MSG_TO_EDIT",
        },
        type: 14, // MESSAGE_EDIT
        editedMessage: {
          conversation: "edited text",
        },
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.protocolMessage.type).toBe(14);
    expect(decoded.protocolMessage.editedMessage.conversation).toBe(
      "edited text"
    );
  });

  test("encode does not corrupt when extra fields are present", () => {
    // Simulate what generateWAMessage produces: message with many empty/null fields
    const msg = {
      extendedTextMessage: {
        text: "hello",
        contextInfo: {
          stanzaId: "ABC",
          participant: "5511999@s.whatsapp.net",
          quotedMessage: null,
        },
      },
      messageContextInfo: {
        messageSecret: new Uint8Array(32),
      },
    };
    const bytes = encodeProto("Message", msg);
    expect(bytes.length).toBeGreaterThan(0);

    const decoded = decodeProto("Message", bytes);
    expect(decoded.extendedTextMessage.text).toBe("hello");
  });

  test("encode with camelCase keys (what Baileys sends)", () => {
    // Baileys uses camelCase, our encodeProto converts to snake_case
    const msg = {
      extendedTextMessage: {
        text: "camelCase test",
        contextInfo: {
          stanzaId: "STANZA1",
          participant: "5511999@s.whatsapp.net",
          isForwarded: true,
          forwardingScore: 1,
        },
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.extendedTextMessage.text).toBe("camelCase test");
    expect(decoded.extendedTextMessage.contextInfo.stanzaId).toBe("STANZA1");
    expect(decoded.extendedTextMessage.contextInfo.isForwarded).toBe(true);
    expect(decoded.extendedTextMessage.contextInfo.forwardingScore).toBe(1);
  });

  test("pollCreationMessageV3", () => {
    const msg = {
      pollCreationMessageV3: {
        name: "Test Poll",
        options: [
          { optionName: "Option 1" },
          { optionName: "Option 2" },
          { optionName: "Option 3" },
        ],
        selectableOptionsCount: 1,
        messageSecret: new Uint8Array(32),
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.pollCreationMessageV3.name).toBe("Test Poll");
    expect(decoded.pollCreationMessageV3.options.length).toBe(3);
  });

  test("contactMessage", () => {
    const msg = {
      contactMessage: {
        displayName: "Test Contact",
        vcard:
          "BEGIN:VCARD\nVERSION:3.0\nFN:Test\nTEL:+1234567890\nEND:VCARD",
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.contactMessage.displayName).toBe("Test Contact");
    expect(decoded.contactMessage.vcard).toContain("FN:Test");
  });

  test("binary roundtrip matches (encode → decode → encode)", () => {
    const msg = {
      extendedTextMessage: {
        text: "roundtrip test",
      },
    };
    const bytes1 = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes1);
    // Re-encode the decoded message (now with camelCase from CamelSerializer)
    const bytes2 = encodeProto("Message", decoded);
    // Both should produce the same proto binary
    expect(bytes1.length).toBe(bytes2.length);
    for (let i = 0; i < bytes1.length; i++) {
      expect(bytes1[i]).toBe(bytes2[i]);
    }
  });
});
