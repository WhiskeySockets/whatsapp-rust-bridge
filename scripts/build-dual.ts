#!/usr/bin/env bun
import { rm } from "fs/promises";
import { spawnSync } from "child_process";

console.log("🔨 Building dual WASM modules (SIMD + baseline)...\n");

// Clean up old builds
await rm("pkg", { recursive: true, force: true });
await rm("pkg-nosimd", { recursive: true, force: true });

// 1. Build SIMD version (default with current config)
console.log("📦 Building SIMD version...");
const simdBuild = spawnSync(
    "wasm-pack",
    ["build", "--target", "web", "--out-dir", "pkg", "--no-pack"],
    {
        stdio: "inherit",
        shell: true,
    },
);
if (simdBuild.status !== 0) {
    process.exit(1);
}
console.log("✅ SIMD build complete\n");

// 2. Build non-SIMD version
console.log("📦 Building baseline (no-SIMD) version...");

// Temporarily modify config
const cargoConfig = await Bun.file(".cargo/config.toml").text();
await Bun.write(
    ".cargo/config.toml",
    cargoConfig.replace(
        'rustflags = ["-C", "target-feature=+simd128"]',
        "# rustflags = []",
    ),
);

// Modify Cargo.toml to disable SIMD in wasm-opt
const cargoToml = await Bun.file("Cargo.toml").text();
const modifiedCargoToml = cargoToml.replace(
    '"--enable-simd",',
    '# "--enable-simd",',
);
await Bun.write("Cargo.toml", modifiedCargoToml);

// Build without SIMD
const noSimdBuild = spawnSync(
    "wasm-pack",
    ["build", "--target", "web", "--out-dir", "pkg-nosimd", "--no-pack"],
    {
        stdio: "inherit",
        shell: true,
    },
);

// Restore configs
await Bun.write(".cargo/config.toml", cargoConfig);
await Bun.write("Cargo.toml", cargoToml);

if (noSimdBuild.status !== 0) {
    process.exit(1);
}
console.log("✅ Baseline build complete\n");

console.log("📊 Build results:");
const simdSize = Bun.file("pkg/whatsapp_rust_bridge_bg.wasm").size;
const noSimdSize = Bun.file("pkg-nosimd/whatsapp_rust_bridge_bg.wasm").size;
console.log(`  SIMD:     ${(simdSize / 1024).toFixed(1)} KB`);
console.log(`  Baseline: ${(noSimdSize / 1024).toFixed(1)} KB`);
console.log(`  Diff:     ${((simdSize - noSimdSize) / 1024).toFixed(1)} KB\n`);

console.log("✨ Dual build complete!");
