# WhatsApp Rust Bridge - AI Coding Guidelines (napi-rs Edition)

## 1. Architecture Overview

This project is an **isomorphic Rust library** built with `napi-rs`. It compiles a single Rust codebase into a high-performance, portable **WebAssembly module** designed to run within the **Node.js environment**. The core goal is to provide a single, easy-to-distribute package that doesn't require native compilation on the end-user's machine.

- **Rust Core (`src/`)**: Contains all the core logic for the Signal Protocol (`libsignal_api.rs`) and binary protocol handling (`wasm_api.rs`). It uses powerful crates like `wacore-libsignal`.
- **FFI Layer (`napi-rs`)**: We use the `#[napi]` macro from `napi-rs` to define the boundary between Rust and JavaScript. This replaces the old `wasm-bindgen` setup.
- **Build System (`@napi-rs/cli`)**: The primary tool is `@napi-rs/cli`. It orchestrates the entire build process by invoking `cargo` with the correct target and flags.
- **WASM Target (`wasm32-wasip1-threads`)**: We exclusively use this target. It provides a POSIX-like environment (WASI) and enables Rust's standard multi-threading (`std::thread`) and `async` runtimes to work out-of-the-box in WebAssembly.
- **JS Loader (`scripts/index.js`)**: We use a custom JavaScript entrypoint to correctly load the compiled `.wasm` file in Node.js using the `@napi-rs/wasm-runtime` package.

## 2. Key Design Patterns

### The `#[napi]` Macro

This is the cornerstone of our FFI. It's used on functions, structs, and `impl` blocks to expose them to JavaScript.

- An `async fn` in Rust automatically becomes a function that returns a `Promise` in JavaScript.
- Structs decorated with `#[napi(object)]` are used for passing plain data objects between JS and Rust.
- `napi-rs` automatically handles conversions for basic types (`String`, numbers), `Buffer` (`Vec<u8>`), and `Result` (which becomes a thrown `Error`).

### Isomorphic Async (`async fn`)

Our `async` Rust functions are executed on a Tokio runtime thread pool managed by `napi-rs`. This has a critical safety implication:

- **`Send` Trait Requirement**: Any data held across an `.await` point **must** be `Send` (safe to move between threads).
- **The Pattern**: `napi-rs` JavaScript types like `Object` or `Function` are **not `Send`**. Therefore, the correct pattern is:
  1.  Perform all JavaScript interactions (calling JS functions, reading object properties) _before_ the first `.await`.
  2.  Extract the data into pure, `Send`-able Rust types (`String`, `Vec<u8>`, etc.).
  3.  Perform the long-running async work using only these Rust types.
  4.  After the final `.await`, interact with JavaScript again if necessary to write results back.

## 3. Build & Development Workflow

### Primary Commands

- **Full WASM Build**: `bun run build`
  - Invokes `napi build --release -t wasm32-wasip1-threads ...`
  - Compiles the Rust code into `dist/index.wasm32-wasi.wasm`.
  - Copies our custom loader `scripts/index.js` to `dist/index.js`.
- **Debug Build**: `bun run build:debug`
- **Test**: `bun test` (runs tests against the artifacts in the `dist` directory).

### WASM-Specific Environment

Building for `wasm32-wasip1-threads` has two strict requirements:

1.  **Nightly Rust Toolchain**: The `rust-toolchain.toml` file enforces this.
2.  **WASI SDK**: The path to the extracted WASI SDK **must** be available in the `WASI_SDK_PATH` environment variable. The `@napi-rs/cli` tool uses this to correctly link C/C++ dependencies.

### Testing Pattern

- We use `bun:test`.
- Since our WASM module initializes asynchronously, tests must `await` the default export from `dist/index.js` before calling any functions. This is typically done in a `beforeAll` block.

```typescript
import { describe, test, expect, beforeAll } from "bun:test";
import apiPromise from "../dist/index.js";

let api;

beforeAll(async () => {
  api = await apiPromise;
});

test("should do something", () => {
  const result = api.someFunction(); // Use the resolved API
  expect(result).toBe(true);
});
```

## 4. Code Organization

- `src/libsignal_api.rs`: FFI for Signal Protocol functions. All functions here are `async` and interact with a JavaScript `store` object.
- `src/wasm_api.rs`: FFI for binary node encoding/decoding.
- `src/lib.rs`: Main library crate that declares the modules.
- `Cargo.toml`: Defines all Rust dependencies. Note the `napi` and `napi-derive` crates and the `cdylib` crate-type.
- `package.json`: Contains the `napi` configuration, which tells the CLI to _only_ build the `wasm32-wasip1-threads` target.
- `scripts/index.js`: **Crucial File**. This is our custom Node.js loader for the WASM module. It uses `@napi-rs/wasm-runtime` to correctly instantiate the module with all required imports (`env` and `wasi_snapshot_preview1`).

## 5. Key Dependencies & Tooling

- **Core FFI**: `@napi-rs/cli`, `napi`, `napi-derive`.
- **WASM Runtime**: `@napi-rs/wasm-runtime` (used in our custom loader).
- **Rust Toolchain**: `nightly` with the `wasm32-wasip1-threads` target.
- **System Dependency**: `wasi-sdk`.
- **Testing**: `bun:test`.
