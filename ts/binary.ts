// Re-export all functions from the napi-rs generated module
export * from "../dist";

export interface INode {
  tag: string;
  attrs: { [key: string]: string };
  content?: INode[] | string | Uint8Array;
}
