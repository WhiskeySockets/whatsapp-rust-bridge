# AI Coding Agent Instructions for whatsapp-rust-bridge

Concise, project-specific guidance for automated agents. Focus on the existing patterns; do not invent new architecture.

## 1. Purpose & Scope
High-performance WhatsApp binary node (tree) marshal / unmarshal utilities compiled from Rust to WebAssembly. JS/TS layer provides ergonomic wrappers and a builder API. Future roadmap hints (README) mention Libsignal but it's not implemented yet—ignore until code exists.

## 2. Core Architecture
- Rust crate (`src/`) exposes a WASM-friendly API via `wasm-bindgen`.
  - Entry: `lib.rs` registers `wee_alloc` (smaller allocator) and re-exports `wasm_api` + `wasm_types`.
  - `wasm_api.rs` is the ONLY Rust <-> JS bridge logic: defines serde-friendly `JsNode`, conversion to/from internal `wacore_binary::Node` & binary marshalling (`marshal_node`, `unmarshal_node`) plus a fluent `WasmNodeBuilder`.
  - `wasm_types.rs` supplies a TypeScript interface (`INode`) via `#[wasm_bindgen(typescript_custom_section)]` – do not duplicate this interface on the TS side; import types from the generated `.d.ts`.
- External dependency `wacore-binary` (Git repo) provides the internal Node structures and `marshal/unmarshal_ref` routines. Treat it as source of truth for binary format (not duplicated here).
- Generated WASM + JS glue emitted to `pkg/` (via `wasm-pack`). Top-level TS wrapper code lives in `ts/` and compiled output ships in `dist/`.

## 3. Public JavaScript / TypeScript Surface
Exposed through `exports` field (`package.json`): `import { init, marshal, unmarshal, NodeBuilder, type INode } from 'whatsapp-rust-bridge/binary'` after build.
Key behaviors:
- `init()` manually reads the `.wasm` file (works in Node & browser) and constructs a `WebAssembly.Module` (bypasses network streaming pitfalls; keeps deterministic env). Always call before other functions in fresh contexts.
- `marshal(node: INode): Uint8Array` passes a plain object to WASM (UTF-8 string content => bytes). Returns binary.
- `unmarshal(data: Uint8Array): INode` NOTE: current implementation slices `data.subarray(1)` before passing to WASM (offset expectation). Preserve this unless underlying binary protocol changes.
- `NodeBuilder` wraps `WasmNodeBuilder`; use for incremental construction; `build()` returns binary (already marshalled). No direct method to get intermediate object—design choice to avoid redundant allocations.

## 4. Conventions & Patterns
- Conversion path: JS object -> serde_wasm_bindgen -> `JsNode` -> internal `Node` -> `wacore_binary::marshal`.
- Attributes stored as `HashMap<String,String>`; omit empty maps (`skip_serializing_if`). Keep attr values as strings—binary payload goes in `content` bytes.
- `content` union semantics:
  - Array => child nodes
  - String => treated as UTF-8 bytes (Rust side converts to `Bytes`)
  - Uint8Array => raw bytes
- Avoid adding new Rust structs duplicating JS types; extend `JsNode` if new fields are required.
- For performance: batch child nodes (see `set_children` does bulk deserialize). Prefer building full arrays rather than iterative single-child calls across the boundary.

## 5. Build & Release Workflow
Rust/WASM build: `bun run build:wasm` (wasm-pack, target web, output -> `pkg/`, WITHOUT packaging). Uses `Cargo.toml [package.metadata.wasm-pack.profile.release]` to pass aggressive `wasm-opt` flags (`-O4`, SIMD, bulk-memory, etc.). Keep these flags when adjusting build.
TypeScript build: `bun run build:ts` (tsc for declarations + bun bundler to `dist/`).
Full build (what publish uses): `bun run build`.
Publishing: `prepublishOnly` hook ensures fresh build. Generated tarball example present (`whatsapp-rust-bridge-0.1.0.tgz`).

## 6. Benchmarks
Located at `benches/binary.ts` using `mitata`. Run via `bun run bench`. Bench assumes prior build (needs `dist/binary.js`). Ensure `init()` awaited before measuring. When changing encode/decode paths, update bench to reflect new scenarios.

## 7. Adding / Modifying APIs
- Add Rust function in `wasm_api.rs` with `#[wasm_bindgen]`, expose ergonomic TS wrapper in `ts/binary.ts`, re-export via `exports` map.
- Update or extend `INode` by editing the TypeScript custom section in `wasm_types.rs`. Re-run `build:wasm` to refresh generated `.d.ts`.
- Maintain memory safety: return `Result<..., JsValue>` and convert internal errors with `JsValue::from_str` for consistent JS exceptions.

## 8. Error Handling
All exported Rust functions map errors into thrown JS exceptions via the wasm-bindgen pattern (tuple return + externref). In TS, consumers should catch exceptions; do not wrap with string parsing—prefer passing errors upward.

## 9. Performance Notes
- `wee_alloc` enabled by default feature for smaller WASM size; keep feature flag if adding optional heavier dependencies.
- Avoid per-node boundary crossings; prefer constructing full subtree and calling `marshal` once.
- Be cautious removing `data.subarray(1)` adjustment in `unmarshal`; verify wire format from `wacore-binary` first.

## 10. When Extending
Before large changes: grep `wasm_api` and ensure new logic doesn't duplicate work that could reside in upstream `wacore-binary`. If needing new binary primitives, implement them upstream then consume here.

## 11. Quick Task Examples
- Add new node property: extend `JsNode` + matching conversion + update `INode` interface.
- Expose round-trip helper: implement Rust fn that accepts bytes and returns bytes or stats (avoid repeated marshal/unmarshal in JS for micro-ops).

## 12. Do NOT
- Do not introduce another allocator while `wee_alloc` active.
- Do not regenerate `pkg/` manually without `wasm-pack`; keep deterministic outputs.
- Do not change export names without updating `exports` map in `package.json`.

Provide succinct PR descriptions referencing section numbers of this document when relevant (e.g., "Implements new field per §7").
