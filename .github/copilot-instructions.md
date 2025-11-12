# WhatsApp Rust Bridge - AI Coding Guidelines

## Architecture Overview

This is a high-performance Rust-WebAssembly bridge for WhatsApp's binary protocol. The core architecture consists of:

- **Rust WASM Core** (`src/`): Zero-copy binary encoding/decoding using `wacore-binary` crate
- **TypeScript/JavaScript Layer** (`ts/`): WASM initialization and API exports

## Key Design Patterns

### Zero-Copy Decoding

- `WasmNode` struct holds references to WASM memory, avoiding copies
- Use `decodeNode()` for lazy access to decoded data
- Memory managed by custom Talc allocator (see `src/lib.rs`)

### Content Type Handling

- **Strings**: Passed as `Uint8Array` in decoded content (always UTF-8 encoded)
- **Binary data**: Direct `Uint8Array` representation
- **Token strings**: Compressed to single bytes (e.g., "receipt" → 5 bytes)
- Distinguish by input type: `string` vs `Uint8Array` in `INode.content`

### Node Structure

```typescript
// For encoding (input)
interface INode {
  tag: string;
  attrs: { [key: string]: string };
  content?: INode[] | string | Uint8Array;
}

// For decoding (output handle)
class WasmNode {
  readonly tag: string;
  readonly children: INode[];
  readonly content?: Uint8Array;
  getAttribute(key: string): string | undefined;
  getAttributes(): { [key: string]: string };
}
```

## Build & Development Workflow

### Primary Commands

- **Full build**: `bun run build` (WASM + TS bundle + declarations)
- **Test**: `bun test` (runs `test/binary.test.ts`)
- **Benchmark**: `bun run bench` (builds then runs `benches/binary.ts`)

### Testing Patterns

- Use Bun's test runner (`bun:test`)
- Round-trip testing: encode → decode → compare
- Content type verification: string vs binary handling
- Error case testing: truncated data, invalid inputs

## Code Organization

### File Structure

- `src/wasm_api.rs` - Core WASM bindings and conversion logic
- `src/wasm_types.rs` - TypeScript type definitions via `typescript_custom_section`
- `ts/binary.ts` - JavaScript entry point with WASM initialization
- `Cargo.toml` - Rust dependencies (note: uses private `wacore-binary` crate)

### Naming Conventions

- Rust: `snake_case` functions, `PascalCase` structs
- JavaScript: `camelCase` functions, `PascalCase` classes/interfaces
- WASM exports: `js_name` attributes for camelCase (e.g., `encode_node` → `encodeNode`)

## Performance Considerations

### Memory Management

- Custom Talc allocator optimized for WASM
- Zero-copy where possible (decoding)
- Reuse buffers with `encodeNodeTo()` for hot paths

### WASM Optimization

- Release profile: `lto = "fat"`, `opt-level = 3`, `codegen-units = 1`
- Custom `wasm-opt` flags for size/speed balance
- Target CPU: `native` for WASM builds

## Common Patterns

### Encoding Flow

```rust
// JS object → Rust Node → binary bytes
let node: Node = js_to_node(js_val)?;
let bytes = marshal(&node)?;
```

### Decoding Flow

```rust
// binary bytes → unpacked → NodeRef → WasmNode handle
let unpacked = unpack(data)?;
let node_ref = unmarshal_ref(unpacked)?;
let handle = WasmNode { _owned_data, node_ref };
```

### Attribute Access

- `getAttribute()` - Single attribute lookup
- `getAttributes()` - All attributes as JS object (single FFI call)
- `getAttributeAsJid()` - JID-specific parsing

## Dependencies & Tooling

- Declaration generation requires specific TypeScript config (`emitDeclarationOnly: true`)
- **Testing**: Bun test runner
- **Benchmarking**: Mitata
- **WASM**: wasm-pack + wasm-bindgen
- **Rust**: 2024 edition, custom allocator

## Gotchas

- WASM memory is static lifetime - careful with references
- Content encoding: strings become `Uint8Array` (not `string`) in decoded output
- Build order matters: WASM must be built before TypeScript bundling
- Declaration generation requires specific TypeScript config (`emitDeclarationOnly: true`)
