/**
 * Type safety test — verifies the generated types are correct and usable.
 * This is a compile-time test (tsc checks), not a runtime test.
 */

import { describe, test, expect } from "bun:test";
import type {
  Event,
  MessageInfo,
  MessageSource,
  Receipt,
  ReceiptType,
  AddressingMode,
  EditAttribute,
  ConnectFailureReason,
  GroupNotificationAction,
  ChatPresence,
  PresenceUpdate,
  WasmWhatsAppClient,
  JsTransportCallbacks,
  JsTransportHandle,
  JsHttpClientConfig,
  WaMessage,
  GroupMetadataResult,
  IsOnWhatsAppResult,
} from "../types/index.js";

describe("Generated TypeScript types", () => {
  test("Event discriminated union is type-safe", () => {
    // This function only compiles if the Event type is a proper discriminated union
    function handleEvent(event: Event) {
      switch (event.type) {
        case "connected":
          // data should be Connected (empty interface)
          break;
        case "pair_success":
          // data has id, lid, businessName, platform
          const ps = event.data;
          const _id: string = ps.id;
          const _lid: string = ps.lid;
          break;
        case "message":
          // message event
          break;
        case "receipt":
          const receipt: Receipt = event.data;
          const _msgIds: string[] = receipt.messageIds;
          break;
        case "logged_out":
          const lo = event.data;
          const _onConnect: boolean = lo.onConnect;
          break;
        case "group_update":
          const gu = event.data;
          const _groupJid: string = gu.groupJid;
          break;
        case "connect_failure":
          const cf = event.data;
          const _reason: ConnectFailureReason = cf.reason;
          break;
      }
    }

    // Just verify the function compiles
    expect(typeof handleEvent).toBe("function");
  });

  test("MessageInfo interface has correct fields", () => {
    const info: MessageInfo = {
      source: {
        chat: "123@s.whatsapp.net",
        sender: "456@s.whatsapp.net",
        isFromMe: false,
        isGroup: false,
      },
      id: "msg123",
      serverId: 1,
      type: "text",
      pushName: "Alice",
      timestamp: Date.now(),
      category: "",
      multicast: false,
      mediaType: "",
      edit: "empty",
    };

    expect(info.source.chat).toBe("123@s.whatsapp.net");
    expect(info.source.isFromMe).toBe(false);
  });

  test("AddressingMode is a string union", () => {
    const mode: AddressingMode = "pn";
    expect(mode).toBe("pn");

    const lidMode: AddressingMode = "lid";
    expect(lidMode).toBe("lid");
  });

  test("WaMessage accepts message content", () => {
    const textMsg: WaMessage = {
      conversation: "Hello from TypeScript!",
    };
    expect(textMsg.conversation).toBe("Hello from TypeScript!");

    const extMsg: WaMessage = {
      extended_text_message: {
        text: "Extended text with URL",
      },
    };
    expect(extMsg.extended_text_message?.text).toBe("Extended text with URL");
  });

  test("JsTransportCallbacks interface is correct", () => {
    const transport: JsTransportCallbacks = {
      connect(handle: JsTransportHandle) {
        handle.onConnected();
        handle.onData(new Uint8Array([1, 2, 3]));
        handle.onDisconnected();
      },
      send(data: Uint8Array) {
        // send bytes
      },
      disconnect() {
        // close
      },
    };

    expect(typeof transport.connect).toBe("function");
    expect(typeof transport.send).toBe("function");
    expect(typeof transport.disconnect).toBe("function");
  });

  test("GroupNotificationAction is a discriminated union", () => {
    function handleAction(action: GroupNotificationAction) {
      switch (action.type) {
        case "add":
          const _participants = action.participants;
          const _reason = action.reason;
          break;
        case "remove":
          break;
        case "promote":
          break;
        case "demote":
          break;
      }
    }

    expect(typeof handleAction).toBe("function");
  });
});
