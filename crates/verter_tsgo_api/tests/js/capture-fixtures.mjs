// Fixture capture harness for the verter_tsgo_api hand-written codec tests.
//
// Captures REAL tsgo `--api` wire bytes (the MessagePack tuple frames) so the
// Rust codec can be asserted byte-for-byte against ground truth produced by the
// shipped engine + the official JS framing. Writes JSON fixtures to
// crates/verter_tsgo_api/tests/fixtures/.
//
// Two kinds of fixtures are produced:
//   1. request frames — the exact bytes the JS `SyncRpcChannel.writeTuple`
//      emits for each op's request (built with the shipped `MsgpackWriter` /
//      `writeBinHeader`). These pin the WRITE side of the Rust codec.
//   2. live frames — the genuine bytes tsgo writes back on the pipe for a real
//      request (a raw capture at the pipe boundary). These pin the READ side
//      against real engine output, not a JS re-encode.
//
// Run from the repo root (engine discovered automatically):
//   node crates/verter_tsgo_api/tests/js/capture-fixtures.mjs
//
// The engine binary is parameterized via TSGO_PATH (else discovered). This
// harness is TEST-ONLY and is never referenced from the production crate.

import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { spawn } from "node:child_process";
import { openSync, readSync, writeSync, closeSync, readdirSync } from "node:fs";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE_DIR = path.join(HERE, "..", "fixtures");
const REPO_ROOT = path.resolve(HERE, "..", "..", "..", "..");
const require = createRequire(import.meta.url);

// ── Resolve the shipped wire-codec modules (the framing source of truth) ─────
const pkgJson = require.resolve("typescript/package.json", {
  paths: [REPO_ROOT],
});
const pkgDir = path.dirname(pkgJson);
const msgpackUrl = new URL(
  "file://" + path.join(pkgDir, "dist/api/node/msgpack.js").replace(/\\/g, "/"),
);
const { MsgpackWriter, writeBinHeader, binHeaderSize, MSGPACK_FIXARRAY3 } = await import(
  msgpackUrl.href
);

// ── MessageType constants (mirror syncChannel.js:15-23) ──────────────────────
const MSG_REQUEST = 1;

// ── Reproduce SyncRpcChannel.writeTuple byte output exactly ──────────────────
// (syncChannel.js:264-317) — used to capture canonical request-frame bytes.
function buildTuple(type, name, payload) {
  const nameBuf = Buffer.from(name, "utf-8");
  const payloadBuf =
    typeof payload === "string" ? Buffer.from(payload, "utf-8") : Buffer.from(payload);
  const headerSize =
    2 + binHeaderSize(nameBuf.length) + nameBuf.length + binHeaderSize(payloadBuf.length);
  const total = headerSize + payloadBuf.length;
  const out = Buffer.allocUnsafe(total);
  let off = 0;
  out[off++] = MSGPACK_FIXARRAY3;
  out[off++] = type;
  off = writeBinHeader(out, off, nameBuf.length);
  nameBuf.copy(out, off);
  off += nameBuf.length;
  off = writeBinHeader(out, off, payloadBuf.length);
  payloadBuf.copy(out, off);
  return out;
}

// ── Discover the rc tsc binary portably (mirrors run-gate.mjs::discoverTsgo) ──
function discoverTsgo() {
  if (process.env.TSGO_PATH) return process.env.TSGO_PATH;
  const exeName = os.platform() === "win32" ? "tsc.exe" : "tsc";
  const pnpmDir = path.join(REPO_ROOT, "node_modules", ".pnpm");
  const candidates = [];
  if (fs.existsSync(pnpmDir)) {
    for (const entry of readdirSync(pnpmDir)) {
      if (!/@typescript\+typescript-/.test(entry)) continue;
      const inner = path.join(pnpmDir, entry, "node_modules");
      if (!fs.existsSync(inner)) continue;
      for (const scope of readdirSync(inner)) {
        const scopeDir = path.join(inner, scope);
        try {
          for (const p of readdirSync(scopeDir)) {
            candidates.push(path.join(scopeDir, p, "lib", exeName));
            candidates.push(path.join(scopeDir, p, "bin", exeName));
          }
        } catch {
          /* not a dir */
        }
      }
    }
  }
  const hit = candidates.find((c) => fs.existsSync(c));
  if (!hit) throw new Error("could not discover tsgo binary; set TSGO_PATH");
  return hit;
}

// ── Capture raw bytes tsgo writes back for a single request ──────────────────
// Spawns tsgo with a Windows named pipe (or POSIX stdio), writes one request
// frame, and reads the raw response bytes from the pipe. No callbacks are
// enabled, so for ops that touch no virtual FS the first frame back is the
// response itself.
function liveExchange(exe, cwd, frame, maxBytes = 4096) {
  const isWindows = process.platform === "win32";
  let child, fd;
  if (isWindows) {
    const pipePath = `\\\\.\\pipe\\tsgo-capture-${process.pid}-${Date.now()}`;
    child = spawn(exe, ["--api", "--cwd", cwd, "--pipe", pipePath], {
      stdio: ["ignore", "ignore", "inherit"],
    });
    const sleepBuf = new Int32Array(new SharedArrayBuffer(4));
    for (let i = 0; i < 500; i++) {
      try {
        fd = openSync(pipePath, "r+");
        break;
      } catch {
        if (child.exitCode !== null) throw new Error("child exited before pipe ready");
        Atomics.wait(sleepBuf, 0, 0, 10);
      }
    }
    if (fd === undefined) {
      child.kill();
      throw new Error("timed out connecting to named pipe");
    }
    writeSync(fd, frame, 0, frame.length);
    const buf = Buffer.allocUnsafe(maxBytes);
    const n = readSync(fd, buf, 0, maxBytes, null);
    closeSync(fd);
    child.kill();
    return buf.subarray(0, n);
  } else {
    child = spawn(exe, ["--api", "--cwd", cwd], { stdio: ["pipe", "pipe", "inherit"] });
    child.stdout._handle.setBlocking?.(true);
    child.stdin._handle.setBlocking?.(true);
    const wfd = child.stdin._handle.fd;
    const rfd = child.stdout._handle.fd;
    writeSync(wfd, frame, 0, frame.length);
    const buf = Buffer.allocUnsafe(maxBytes);
    const n = readSync(rfd, buf, 0, maxBytes, null);
    child.kill();
    return buf.subarray(0, n);
  }
}

function toHex(buf) {
  return Buffer.from(buf).toString("hex");
}

// ── Build the request-frame fixtures (canonical WRITE bytes) ─────────────────
const requestFixtures = [
  { name: "initialize", method: "initialize", payload: "null" },
  {
    name: "updateSnapshot_openProject",
    method: "updateSnapshot",
    payload: JSON.stringify({ openProject: "/repo/tsconfig.json" }),
  },
  {
    name: "updateSnapshot_fileChanges",
    method: "updateSnapshot",
    payload: JSON.stringify({
      openProject: "/repo/tsconfig.json",
      fileChanges: { changed: ["/repo/src/a.ts"] },
    }),
  },
  {
    name: "getSemanticDiagnostics",
    method: "getSemanticDiagnostics",
    payload: JSON.stringify({
      snapshot: 1,
      project: "p.x",
      file: "/repo/src/a.ts",
    }),
  },
  {
    name: "getTypeAtPosition",
    method: "getTypeAtPosition",
    payload: JSON.stringify({
      snapshot: 1,
      project: "p.x",
      file: "/repo/src/a.ts",
      position: 42,
    }),
  },
  {
    name: "getSymbolAtPosition",
    method: "getSymbolAtPosition",
    payload: JSON.stringify({
      snapshot: 1,
      project: "p.x",
      file: "/repo/src/a.ts",
      position: 42,
    }),
  },
  {
    name: "typeToString",
    method: "typeToString",
    payload: JSON.stringify({ snapshot: 1, project: "p.x", type: 1 }),
  },
  { name: "echo", method: "echo", payload: "hello-verter" },
];

const out = {
  engineVersion: JSON.parse(fs.readFileSync(pkgJson, "utf8")).version,
  note: "Canonical tsgo --api tuple frames. requestFrames are JS-channel WRITE bytes; liveFrames are raw bytes tsgo wrote back. Hex-encoded.",
  requestFrames: {},
  liveFrames: {},
};

for (const f of requestFixtures) {
  const frame = buildTuple(MSG_REQUEST, f.method, f.payload);
  out.requestFrames[f.name] = {
    method: f.method,
    payload: f.payload,
    hex: toHex(frame),
  };
}

// ── Capture a genuine live response frame (READ-side ground truth) ───────────
// `initialize` needs no project/FS and returns a small response; it is the
// cleanest live-frame capture.
try {
  const exe = discoverTsgo();
  const initFrame = buildTuple(MSG_REQUEST, "initialize", "null");
  const raw = liveExchange(exe, REPO_ROOT, initFrame);
  out.liveFrames["initialize_response"] = {
    request_method: "initialize",
    hex: toHex(raw),
    captured_from: "real tsgo engine",
  };
  console.log(`captured live initialize response: ${raw.length} bytes`);
} catch (e) {
  out.liveFrames["__capture_error"] = String(e && e.message ? e.message : e);
  console.error("live capture failed (request fixtures still written):", e.message);
}

fs.mkdirSync(FIXTURE_DIR, { recursive: true });
const outPath = path.join(FIXTURE_DIR, "wire-frames.json");
fs.writeFileSync(outPath, JSON.stringify(out, null, 2) + "\n", "utf8");
console.log("wrote", outPath.replace(/\\/g, "/"));
