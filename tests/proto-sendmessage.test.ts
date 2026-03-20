/**
 * Tests that the sendMessage serde path correctly handles camelCase → snake_case.
 * This caught a critical bug where sendMessage received camelCase keys but
 * serde expected snake_case, silently producing empty messages.
 *
 * Run: bun test tests/proto-sendmessage.test.ts
 */

import { describe, test, expect, beforeAll } from "bun:test";
import {
  initWasmEngine,
  createWhatsAppClient,
  encodeProto,
  decodeProto,
} from "../dist/index.js";

beforeAll(() => {
  initWasmEngine();
});

describe("sendMessage serde path (camelCase → snake_case)", () => {
  test("encodeProto handles camelCase extendedTextMessage", () => {
    // This is what Baileys generates (camelCase)
    const msg = { extendedTextMessage: { text: "hello" } };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.extendedTextMessage.text).toBe("hello");
  });

  test("encodeProto handles camelCase contextInfo inside extendedTextMessage", () => {
    const msg = {
      extendedTextMessage: {
        text: "reply text",
        contextInfo: {
          stanzaId: "ABC123",
          participant: "5511999@s.whatsapp.net",
          isForwarded: true,
          forwardingScore: 2,
        },
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.extendedTextMessage.text).toBe("reply text");
    expect(decoded.extendedTextMessage.contextInfo.stanzaId).toBe("ABC123");
    expect(decoded.extendedTextMessage.contextInfo.isForwarded).toBe(true);
    expect(decoded.extendedTextMessage.contextInfo.forwardingScore).toBe(2);
  });

  test("encodeProto handles camelCase imageMessage with bytes", () => {
    const mediaKey = new Uint8Array(32);
    crypto.getRandomValues(mediaKey);

    const msg = {
      imageMessage: {
        url: "https://mmg.whatsapp.net/test",
        mimetype: "image/jpeg",
        caption: "photo",
        fileSha256: new Uint8Array(32),
        fileLength: 5000,
        mediaKey: mediaKey,
        fileEncSha256: new Uint8Array(32),
        directPath: "/mms/image/hash",
        jpegThumbnail: new Uint8Array(100),
      },
    };
    const bytes = encodeProto("Message", msg);
    expect(bytes.length).toBeGreaterThan(0);

    const decoded = decodeProto("Message", bytes);
    expect(decoded.imageMessage.caption).toBe("photo");
    expect(decoded.imageMessage.mediaKey).toBeInstanceOf(Uint8Array);
    expect(decoded.imageMessage.mediaKey.length).toBe(32);
    expect(decoded.imageMessage.jpegThumbnail).toBeInstanceOf(Uint8Array);
    expect(decoded.imageMessage.jpegThumbnail.length).toBe(100);
  });

  test("encodeProto handles camelCase videoMessage", () => {
    const msg = {
      videoMessage: {
        url: "https://mmg.whatsapp.net/video",
        mimetype: "video/mp4",
        caption: "video",
        seconds: 42,
        mediaKey: new Uint8Array(32),
        fileEncSha256: new Uint8Array(32),
        fileSha256: new Uint8Array(32),
        gifPlayback: false,
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.videoMessage.caption).toBe("video");
    expect(decoded.videoMessage.seconds).toBe(42);
  });

  test("encodeProto handles camelCase audioMessage with ptt", () => {
    const msg = {
      audioMessage: {
        url: "https://mmg.whatsapp.net/audio",
        mimetype: "audio/ogg; codecs=opus",
        seconds: 10,
        ptt: true,
        mediaKey: new Uint8Array(32),
        fileEncSha256: new Uint8Array(32),
        fileSha256: new Uint8Array(32),
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.audioMessage.ptt).toBe(true);
    expect(decoded.audioMessage.seconds).toBe(10);
  });

  test("encodeProto handles camelCase documentMessage", () => {
    const msg = {
      documentMessage: {
        url: "https://mmg.whatsapp.net/doc",
        mimetype: "application/pdf",
        title: "test.pdf",
        fileName: "test.pdf",
        mediaKey: new Uint8Array(32),
        fileEncSha256: new Uint8Array(32),
        fileSha256: new Uint8Array(32),
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.documentMessage.title).toBe("test.pdf");
    expect(decoded.documentMessage.fileName).toBe("test.pdf");
  });

  test("encodeProto handles camelCase reactionMessage", () => {
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
    expect(decoded.reactionMessage.key.remoteJid).toBe(
      "5511999@s.whatsapp.net"
    );
  });

  test("encodeProto handles camelCase protocolMessage REVOKE", () => {
    const msg = {
      protocolMessage: {
        key: {
          remoteJid: "5511999@s.whatsapp.net",
          fromMe: true,
          id: "DEL123",
        },
        type: 0,
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.protocolMessage.key.id).toBe("DEL123");
  });

  test("encodeProto handles camelCase protocolMessage MESSAGE_EDIT", () => {
    const msg = {
      protocolMessage: {
        key: {
          remoteJid: "5511999@s.whatsapp.net",
          fromMe: true,
          id: "EDIT123",
        },
        type: 14,
        editedMessage: {
          extendedTextMessage: {
            text: "edited text",
          },
        },
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.protocolMessage.type).toBe(14);
    expect(
      decoded.protocolMessage.editedMessage.extendedTextMessage.text
    ).toBe("edited text");
  });

  test("encodeProto handles camelCase pollCreationMessageV3", () => {
    const msg = {
      pollCreationMessageV3: {
        name: "Poll",
        options: [{ optionName: "A" }, { optionName: "B" }],
        selectableOptionsCount: 1,
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.pollCreationMessageV3.name).toBe("Poll");
    expect(decoded.pollCreationMessageV3.options.length).toBe(2);
    expect(decoded.pollCreationMessageV3.options[0].optionName).toBe("A");
  });

  test("encodeProto handles camelCase contactMessage", () => {
    const msg = {
      contactMessage: {
        displayName: "Test",
        vcard: "BEGIN:VCARD\nVERSION:3.0\nFN:Test\nEND:VCARD",
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.contactMessage.displayName).toBe("Test");
  });

  test("encodeProto handles Buffer objects for bytes fields", () => {
    const msg = {
      imageMessage: {
        url: "https://test",
        mediaKey: Buffer.alloc(32, 0xab),
        fileSha256: Buffer.alloc(32, 0xcd),
        fileEncSha256: Buffer.alloc(32, 0xef),
      },
    };
    const bytes = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.imageMessage.mediaKey).toBeInstanceOf(Uint8Array);
    expect(decoded.imageMessage.mediaKey[0]).toBe(0xab);
    expect(decoded.imageMessage.fileSha256[0]).toBe(0xcd);
  });

  test("encode → decode → encode produces identical proto bytes", () => {
    const msg = {
      extendedTextMessage: {
        text: "roundtrip",
        contextInfo: {
          stanzaId: "XYZ",
          participant: "551@s.whatsapp.net",
        },
      },
    };
    const bytes1 = encodeProto("Message", msg);
    const decoded = decodeProto("Message", bytes1);
    const bytes2 = encodeProto("Message", decoded);
    expect(Buffer.from(bytes1).toString("hex")).toBe(
      Buffer.from(bytes2).toString("hex")
    );
  });

  test("empty message encodes without error", () => {
    const bytes = encodeProto("Message", {});
    expect(bytes.length).toBe(0); // empty proto = 0 bytes
  });

  test("conversation field encodes correctly", () => {
    const msg = { conversation: "simple text" };
    const bytes = encodeProto("Message", msg);
    // Field 1 (conversation), wire type 2 = 0x0a
    expect(bytes[0]).toBe(0x0a);
    const decoded = decodeProto("Message", bytes);
    expect(decoded.conversation).toBe("simple text");
  });
});
