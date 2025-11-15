export class SessionRecord {
  private data: Uint8Array;

  constructor(serialized?: Uint8Array) {
    this.data = serialized || new Uint8Array(0);
  }

  static deserialize(data: Uint8Array): SessionRecord {
    return new SessionRecord(data);
  }

  serialize(): Uint8Array {
    return this.data;
  }
}
