/**
 * Type safety test — verifies the generated types are correct and usable.
 * This is a compile-time test (tsc checks), not a runtime test.
 */

import { describe, test, expect } from "bun:test";
import type {
  WhatsAppEvent,
  MessageInfo,
  MessageSource,
  Receipt,
  ReceiptType,
  AddressingMode,
  EditAttribute,
  ConnectFailureReason,
  GroupNotificationAction,
  GroupUpdate,
  ChatPresence,
  ChatPresenceUpdate,
  PresenceUpdate,
  PictureUpdate,
  UndecryptableMessage,
  OfflineSyncPreview,
  ConnectFailure,
  JsTransportCallbacks,
  JsTransportHandle,
  JsHttpClientConfig,
  WaMessage,
  GroupMetadataResult,
  IsOnWhatsAppResult,
} from "../pkg/whatsapp_rust_bridge.js";

describe("Generated TypeScript types", () => {
  test("WhatsAppEvent discriminated union is fully typed", () => {
    // This function only compiles if all event data types are correct
    function handleEvent(event: WhatsAppEvent) {
      switch (event.type) {
        case "connected":
          // data is Record<string, never> — no accessible fields
          break;
        case "pair_success":
          const _id: string = event.data.id;
          const _lid: string = event.data.lid;
          const _biz: string = event.data.businessName;
          const _plat: string = event.data.platform;
          break;
        case "message":
          // data.info is typed MessageInfo, data.message is Record<string, unknown>
          const info: MessageInfo = event.data.info;
          const _sender: string = info.source.sender;
          const _chat: string = info.source.chat;
          const _isFromMe: boolean = info.source.isFromMe;
          break;
        case "receipt":
          const receipt: Receipt = event.data;
          const _msgIds: string[] = receipt.messageIds;
          break;
        case "logged_out":
          const _onConnect: boolean = event.data.onConnect;
          const _reason: string = event.data.reason;
          break;
        case "group_update":
          const gu: GroupUpdate = event.data;
          const _groupJid: string = gu.groupJid;
          break;
        case "connect_failure":
          const cf: ConnectFailure = event.data;
          const _cfReason: ConnectFailureReason = cf.reason;
          break;
        case "chat_presence":
          const cp: ChatPresenceUpdate = event.data;
          const _cpState: string = cp.state;
          break;
        case "presence":
          const pu: PresenceUpdate = event.data;
          const _puFrom: string = pu.from;
          break;
        case "picture_update":
          const pic: PictureUpdate = event.data;
          const _picJid: string = pic.jid;
          break;
        case "undecryptable_message":
          const um: UndecryptableMessage = event.data;
          const _umInfo: MessageInfo = um.info;
          break;
        case "offline_sync_preview":
          const osp: OfflineSyncPreview = event.data;
          const _total: number = osp.total;
          break;
        case "offline_sync_completed":
          const _count: number = event.data.count;
          break;
        case "qr":
          const _code: string = event.data.code;
          const _timeout: number = event.data.timeout;
          break;
      }
    }

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
