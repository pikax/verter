#!/usr/bin/env node
/**
 * Downloads tsgo WASM binary and Go wasm_exec.js runtime into public/.
 * Runs automatically before `dev` and `build` via package.json scripts.
 * Skips download if files already exist.
 */
import { existsSync, mkdirSync, writeFileSync, copyFileSync, rmSync, statSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { execSync } from "child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const publicDir = resolve(__dirname, "../public").replace(/\\/g, "/");

const WASM_EXEC_URL =
  "https://raw.githubusercontent.com/golang/go/master/lib/wasm/wasm_exec.js";

if (!existsSync(publicDir)) {
  mkdirSync(publicDir, { recursive: true });
}

async function fetchWasmExec() {
  const dest = resolve(publicDir, "wasm_exec.js");
  if (existsSync(dest)) {
    console.log("[fetch-tsgo] wasm_exec.js already exists, skipping");
    return;
  }
  console.log("[fetch-tsgo] Downloading wasm_exec.js...");
  const res = await fetch(WASM_EXEC_URL);
  if (!res.ok) throw new Error(`Failed to fetch wasm_exec.js: ${res.status}`);
  const buf = Buffer.from(await res.arrayBuffer());
  writeFileSync(dest, buf);
  console.log(`[fetch-tsgo] wasm_exec.js saved (${(buf.length / 1024).toFixed(0)} KB)`);
}

async function fetchTsgoWasm() {
  const dest = resolve(publicDir, "tsgo.wasm");
  if (existsSync(dest)) {
    console.log("[fetch-tsgo] tsgo.wasm already exists, skipping");
    return;
  }

  // Use npm pack to download, then extract with tar using forward-slash paths
  const tmpDir = resolve(__dirname, "../.tsgo-tmp").replace(/\\/g, "/");
  try {
    mkdirSync(tmpDir, { recursive: true });

    console.log("[fetch-tsgo] Downloading tsgo-wasm package...");
    execSync("npm pack tsgo-wasm@latest", { cwd: tmpDir, stdio: "pipe" });

    // Find the tgz file
    const { readdirSync } = await import("fs");
    const tgzFile = readdirSync(tmpDir).find((f) => f.endsWith(".tgz"));
    if (!tgzFile) throw new Error("No .tgz file found after npm pack");

    execSync(`tar -xzf "${tgzFile}"`, { cwd: tmpDir, stdio: "pipe" });

    const extracted = `${tmpDir}/package/tsgo.wasm`;
    if (!existsSync(extracted)) {
      throw new Error("tsgo.wasm not found in extracted package");
    }

    copyFileSync(extracted, dest);
    const size = statSync(dest).size;
    console.log(`[fetch-tsgo] tsgo.wasm saved (${(size / 1024 / 1024).toFixed(1)} MB)`);
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
}

try {
  await Promise.all([fetchTsgoWasm(), fetchWasmExec()]);
  console.log("[fetch-tsgo] Done");
} catch (err) {
  console.error("[fetch-tsgo] Warning:", err.message);
  console.error("[fetch-tsgo] tsgo features may not work in the playground");
  // Don't fail the build — tsgo is optional
  process.exit(0);
}
