import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { EnsuredCorpus } from "./corpus.js";
import {
  warmLspIncremental,
  singleFileEditLatency,
  ideQueryLatency,
  type Workload,
  type WorkloadContext,
} from "./workloads.js";

// The axis-B interactive workloads all gate on the SAME persistent LSP binary via
// the shared `lspAvailable` predicate. The Rust crate is `verter_lsp` (underscore)
// but its `[[bin]]` is `verter-lsp` (HYPHEN — crates/verter_lsp/Cargo.toml), so the
// built artifact on every platform is `verter-lsp[.exe]`. The harness MUST look the
// binary up by that real built name; looking up the underscore crate name silently
// reports the workload unavailable even when the binary was built, so the gate's
// headline interactive checks (warm-lsp-incremental, single-file-edit-latency,
// ide-query-latency) skip vacuously. These specs pin resolution to the real built name and
// discriminate against the old underscore lookup.
const IS_WIN = process.platform === "win32";
const EXE = IS_WIN ? ".exe" : "";

const LSP_WORKLOADS: readonly Workload[] = [
  warmLspIncremental,
  singleFileEditLatency,
  ideQueryLatency,
];

/** A WorkloadContext whose only meaningful field for `available` is `binDir`. */
function ctxWithBinDir(binDir: string): WorkloadContext {
  // `lspAvailable` reads ONLY `ctx.binDir`; the corpus is never touched by the
  // availability probe, so a typed stub is sufficient (and keeps the test
  // hermetic — `findBinary` with a `binDir` set looks ONLY inside it, never at
  // the real target/{debug,release} trees).
  return { binDir, threads: 1, corpus: {} as EnsuredCorpus };
}

/** Materialize a fake executable named `<base><EXE>` inside `binDir`. */
function writeFakeBinary(binDir: string, base: string): void {
  // `findBinary` only does existsSync + statSync(mtime); content/mode are
  // irrelevant, so a placeholder file is a faithful stand-in for the real binary.
  writeFileSync(join(binDir, `${base}${EXE}`), IS_WIN ? "@echo off\r\n" : "#!/bin/sh\n");
}

describe("LSP perf-workload binary resolution", () => {
  let binDir: string;

  beforeEach(() => {
    binDir = mkdtempSync(join(tmpdir(), "verter-lsp-binres-"));
  });

  afterEach(() => {
    rmSync(binDir, { recursive: true, force: true });
  });

  it("resolves the LSP binary by its real built name 'verter-lsp' (hyphen)", () => {
    writeFakeBinary(binDir, "verter-lsp");
    for (const w of LSP_WORKLOADS) {
      const res = w.available(ctxWithBinDir(binDir));
      expect(res.ok, `${w.id} should find the built verter-lsp binary`).toBe(true);
      expect(res.reason, `${w.id} should report no reason when available`).toBeUndefined();
    }
  });

  it("does NOT accept the legacy underscore name 'verter_lsp' (the vacuous-skip bug)", () => {
    // Only the OLD underscore name is present. The harness must NOT look up the
    // underscore crate name and report the workload AVAILABLE — that would let the
    // gate's headline interactive checks "run" against a name the real build never
    // produces. The underscore name is correctly NOT the LSP binary.
    writeFakeBinary(binDir, "verter_lsp");
    for (const w of LSP_WORKLOADS) {
      const res = w.available(ctxWithBinDir(binDir));
      expect(res.ok, `${w.id} must NOT resolve the legacy underscore name`).toBe(false);
      expect(res.reason, `${w.id} reason must name the real hyphen binary`).toMatch(
        /verter-lsp binary not found/,
      );
    }
  });

  it("reports unavailable with the hyphen build hint when no binary exists", () => {
    for (const w of LSP_WORKLOADS) {
      const res = w.available(ctxWithBinDir(binDir));
      expect(res.ok, `${w.id} should be unavailable in an empty binDir`).toBe(false);
      expect(res.reason ?? "", `${w.id} reason must reference verter-lsp`).toContain(
        "verter-lsp binary not found",
      );
    }
  });

  it("is platform-correct: the .exe suffix is required on win32, absent elsewhere", () => {
    // The WRONG-suffix file must NOT satisfy resolution: on win32 a bare
    // `verter-lsp` (no .exe) is not the executable; off-win32 a `verter-lsp.exe`
    // is not the native binary. Guards the cross-platform suffix handling.
    const wrongSuffix = IS_WIN ? "verter-lsp" : "verter-lsp.exe";
    writeFileSync(join(binDir, wrongSuffix), "placeholder");
    for (const w of LSP_WORKLOADS) {
      const res = w.available(ctxWithBinDir(binDir));
      expect(res.ok, `${w.id} must not accept a wrong-suffix binary on this platform`).toBe(false);
    }
  });
});
