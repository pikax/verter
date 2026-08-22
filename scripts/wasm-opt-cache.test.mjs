// Self-tests for scripts/wasm-opt-cache.mjs — a Vitest suite collected by
// the root vitest config, e.g.:
//   pnpm vitest --run scripts/wasm-opt-cache.test.mjs
//
// Covers three fixes from the round-3/round-4 reviews:
//   1. The Windows-`.cmd`/`.bat`-shim spawn path is shell-injection-safe,
//      with the double-escape decision made STRUCTURALLY (reading the
//      shim's own body for a `%*` argument relay), not guessed from the
//      shim's PATH location.
//   2. The actual optimizer binary's content hash reaches the final cache
//      digest on BOTH Windows (`.cmd`/`.bat`) and POSIX (`#!/bin/sh`) shim
//      shapes — not just wasm-opt's reported `--version` string, and not
//      just the Windows case.
//   3. (libc fail-closed lives in packages/*/src/rustHostTriple.test.ts —
//      not this file.)
//
// The Finding-1 tests below assert on the CONSTRUCTED invocation (command
// string / argv / shell flag) and never spawn a real subprocess or touch
// cmd.exe (this suite runs on non-Windows CI). The Finding-2 POSIX test DOES
// touch real temp files on disk (fixture shim files) since it must exercise
// `resolveActualOptimizerFile` → `hashFile` → `computeCacheDigest` end to
// end against a real shim SHAPE, not a synthetic hash.

import { afterEach, describe, expect, it, vi } from "vitest";
import path from "node:path";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";

// A handful of tests below mock `process.platform` to "win32" while creating
// REAL fixture files on this (POSIX) test host, so they can exercise
// `inspectWindowsShim`'s `existsSync(targetPath)` check against a genuine
// shim body. `path.win32.resolve` normalizes its output to backslashes —
// which the real POSIX filesystem treats as literal filename characters, not
// separators — so the win32-shaped resolved target string can never
// `existsSync`-match the real POSIX fixture file underneath it. This mock
// bridges exactly that gap: it defers to the REAL `existsSync` for every
// path except a small allowlist the tests populate with the EXACT win32-
// resolved target string production code computed, so a test still proves
// production's own resolution logic produced the right string — it merely
// stops requiring the OS to independently agree that string denotes a real
// file, which is not achievable while impersonating a foreign platform.
// `readFileSyncCalls` records every path passed to `readFileSync` while the
// mock is active — used below (Finding 3, round 6) to PROVE
// `resolveOptimizerIdentity` reads a shim's body exactly once per call,
// rather than merely asserting its return shape (which an internal
// double-read regression — calling `resolveActualOptimizerFile` +
// `determineDoubleEscape` separately again — would still satisfy).
// `renameSyncCalls` records every (oldPath, newPath) pair passed to
// `renameSync` while the mock is active — used below (Finding 3, round 6) to
// PROVE the uncached fallback writes through a distinct temp file and
// atomically renames it into place, rather than writing directly to the
// final output path (which would skip `renameSync` entirely).
// `copyFileSyncFailFor`/`renameSyncFailFor` are Sets of DESTINATION paths for
// which the mocked call throws instead of delegating to the real fs call —
// used below (round 8) to simulate a cache-population failure (disk full,
// EIO, permissions) that must never prevent the real, correct output from
// being delivered to `outputPath`. `rmSyncFailFor` is the same shape for
// `rmSync` — used below (round 9) to simulate the cache-population cleanup
// (`rmSync(tmpCacheEntry, { force: true })`) ITSELF failing with a genuine
// I/O error (which `force: true` does not suppress), on top of the
// cache-population failure it is cleaning up after.
const {
  fsExtraExisting,
  readFileSyncCalls,
  renameSyncCalls,
  copyFileSyncFailFor,
  renameSyncFailFor,
  rmSyncFailFor,
} = vi.hoisted(() => ({
  fsExtraExisting: new Set(),
  readFileSyncCalls: [],
  renameSyncCalls: [],
  copyFileSyncFailFor: new Set(),
  renameSyncFailFor: new Set(),
  rmSyncFailFor: new Set(),
}));

vi.mock("node:fs", async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    existsSync: (p) => fsExtraExisting.has(p) || actual.existsSync(p),
    readFileSync: (...args) => {
      readFileSyncCalls.push(args[0]);
      return actual.readFileSync(...args);
    },
    copyFileSync: (...args) => {
      if (copyFileSyncFailFor.has(args[1])) {
        throw new Error("EIO: simulated cache copy failure");
      }
      return actual.copyFileSync(...args);
    },
    renameSync: (...args) => {
      renameSyncCalls.push(args);
      if (renameSyncFailFor.has(args[1])) {
        throw new Error("EIO: simulated cache rename failure");
      }
      return actual.renameSync(...args);
    },
    rmSync: (...args) => {
      if (rmSyncFailFor.has(args[0])) {
        throw new Error("EIO: simulated cleanup failure");
      }
      return actual.rmSync(...args);
    },
  };
});

import {
  buildWindowsShimInvocation,
  computeCacheDigest,
  determineDoubleEscape,
  escapeCmdArgument,
  escapeCmdMetaChars,
  extractPosixShimTargetPath,
  extractShimTargetPath,
  finalizeCacheMissOutput,
  hashFile,
  isPosixShellShim,
  isWindowsShim,
  resolveActualOptimizerFile,
  resolveInvocation,
  resolveOptimizerIdentity,
  runUncachedFallback,
  shimRelaysArgsViaPercentStar,
  spawnOptimizerToTempOutput,
} from "./wasm-opt-cache.mjs";

// --- Finding 1: Windows-shim spawn is escaping-safe -------------------------

describe("escapeCmdMetaChars", () => {
  it("escapes every cmd.exe metacharacter with a caret", () => {
    expect(escapeCmdMetaChars("&")).toBe("^&");
    expect(escapeCmdMetaChars("|")).toBe("^|");
    expect(escapeCmdMetaChars("^")).toBe("^^");
    expect(escapeCmdMetaChars("%")).toBe("^%");
    expect(escapeCmdMetaChars("<")).toBe("^<");
    expect(escapeCmdMetaChars(">")).toBe("^>");
    expect(escapeCmdMetaChars('"')).toBe('^"');
  });

  it("leaves ordinary characters untouched", () => {
    expect(escapeCmdMetaChars("hello-world_1.wasm")).toBe("hello-world_1.wasm");
  });
});

// Pure inverse of `escapeCmdMetaChars` — removes ONE layer of caret-escaping
// (`^X` -> `X` for each metachar class member). Used below to simulate what
// a real cmd.exe parse pass does when it consumes a caret-escape layer, so
// the round-trip tests below prove actual parser correctness rather than
// just "a caret appears before the character".
function unescapeCmdMetaCharsOnce(text) {
  return text.replace(/\^([()[\]%!^"`<>&|;, *?])/g, "$1");
}

// Reverses `escapeCmdArgument` for an argument that contains no backslashes
// (every adversarial argument used below is chosen to avoid backslashes, so
// this stays a precise, unambiguous inverse rather than a general re-parser):
// peel `layers` caret-escape passes, strip the outer quote-wrap, then turn
// each escaped `\"` back into a literal `"`.
function unescapeCmdArgument(escaped, layers) {
  let s = escaped;
  for (let i = 0; i < layers; i++) s = unescapeCmdMetaCharsOnce(s);
  if (!s.startsWith('"') || !s.endsWith('"')) {
    throw new Error(
      `expected a quote-wrapped argument after ${layers} unescape pass(es), got: ${s}`,
    );
  }
  s = s.slice(1, -1);
  return s.replace(/\\"/g, '"');
}

describe("escapeCmdArgument", () => {
  it("quotes AND caret-escapes a plain argument (the quotes themselves are cmd.exe metachars)", () => {
    // `"` is itself in the cmd.exe metachar class, so even the wrapping
    // quotes end up caret-escaped — this matches cross-spawn's own
    // `escapeArgument` output exactly (verified against that implementation).
    expect(escapeCmdArgument("-Os", false)).toBe('^"-Os^"');
  });

  it("escapes an embedded ampersand so it cannot terminate/chain a command", () => {
    const escaped = escapeCmdArgument("evil & calc.exe", false);
    // The raw metachar must never survive un-escaped inside the argument.
    expect(escaped).not.toMatch(/(?<!\^)&/);
    expect(escaped).toBe('^"evil^ ^&^ calc.exe^"');
  });

  it("escapes an embedded double quote", () => {
    const escaped = escapeCmdArgument('a"b', false);
    expect(escaped).toBe('^"a\\^"b^"');
  });

  it("escapes a percent sign (would otherwise trigger %VAR% expansion)", () => {
    const escaped = escapeCmdArgument("C:\\Users\\%TEMP%\\out.wasm", false);
    expect(escaped).not.toMatch(/(?<!\^)%/);
  });

  it("escapes a caret (doubled, since ^ is itself the escape character)", () => {
    const escaped = escapeCmdArgument("weird^arg", false);
    expect(escaped).toBe('^"weird^^arg^"');
  });

  it("double-escapes meta chars when doubleEscapeMetaChars is set (a %*-relaying shim)", () => {
    const single = escapeCmdArgument("a&b", false);
    const doubled = escapeCmdArgument("a&b", true);
    expect(doubled).toBe(escapeCmdMetaChars(single));
    expect(doubled).not.toBe(single);
  });

  // --- Genuine round-trip proof through BOTH parser layers ----------------
  //
  // The bare "a caret appears before the character" assertions above prove
  // escaping HAPPENED, not that it survives a real cmd.exe parse. These
  // tests simulate the actual two-stage consumption: stage 1 is the outer
  // `cmd.exe /d /s /c "<command>"` invocation cmd.exe parses to launch a
  // `.cmd`/`.bat` shim (consumes ONE caret-escape layer); stage 2 is the
  // shim's OWN body re-parsing its `%*` expansion when it runs inside the
  // nested cmd.exe batch interpreter (consumes a SECOND layer only for a
  // shim shaped to relay via `%*`). A quote-plus-metacharacter argument
  // (the classic cmd-injection primitive named in the finding) must survive
  // exactly the number of parse layers it will actually go through.
  it("round-trips a quote+ampersand injection argument through a SINGLE parser layer (non-%*-relaying shim)", () => {
    const malicious = 'evil" & calc.exe & "pwned';
    const escaped = escapeCmdArgument(malicious, false);
    expect(unescapeCmdArgument(escaped, 1)).toBe(malicious);
  });

  it("round-trips a quote+ampersand injection argument through BOTH parser layers (a %*-relaying shim)", () => {
    const malicious = 'evil" & calc.exe & "pwned';
    const escaped = escapeCmdArgument(malicious, true);
    expect(unescapeCmdArgument(escaped, 2)).toBe(malicious);
  });

  it("a single real parser pass over a DOUBLE-escaped argument does NOT recover the original — proves the layer count must match the real shim shape", () => {
    const malicious = 'evil" & calc.exe & "pwned';
    const overEscaped = escapeCmdArgument(malicious, true); // wrong layer count for a 1-parse shim
    // A shim whose real invocation only performs ONE cmd.exe reparse leaves
    // this string still wrapped in stray, uninterpreted `^` characters — it
    // is neither the original argument nor a cleanly quote-wrapped one.
    const afterOneRealPass = unescapeCmdMetaCharsOnce(overEscaped);
    expect(afterOneRealPass).not.toBe(malicious);
    expect(afterOneRealPass).toContain("^");
  });

  it("over-escaping (always double-escaping) corrupts the argument actually received by a shim whose real parse only removes ONE layer — proves unconditional double-escaping would NOT be merely safe, but WRONG for a non-relaying shim", () => {
    const malicious = 'evil" & calc.exe & "pwned';
    const correctlyEscaped = escapeCmdArgument(malicious, false); // right call: this shim relays once
    const overEscaped = escapeCmdArgument(malicious, true); // wrong call: "always double-escape" policy

    // Sanity: the correct single-escape form round-trips cleanly through the
    // shim's one real reparse.
    expect(unescapeCmdArgument(correctlyEscaped, 1)).toBe(malicious);

    // The over-escaped form, run through that SAME single real reparse (the
    // shim doesn't know or care what policy produced its argv — it only
    // ever does the one reparse its own body performs), still carries a
    // residual caret-escape layer: the shim receives a corrupted string,
    // not the safe original argument and not even the correctly-escaped
    // intermediate form.
    const corrupted = unescapeCmdMetaCharsOnce(overEscaped);
    expect(corrupted).not.toBe(malicious);
    expect(corrupted).not.toBe(unescapeCmdMetaCharsOnce(correctlyEscaped));
    expect(corrupted).toContain("^");
  });
});

describe("buildWindowsShimInvocation", () => {
  const shimPath = "C:\\proj\\node_modules\\.bin\\wasm-opt.cmd";

  it("returns args: [] and shell: true — never a raw args array under shell:true", () => {
    const invocation = buildWindowsShimInvocation(shimPath, ["-Os"], false);
    expect(invocation.args).toEqual([]);
    expect(invocation.shell).toBe(true);
    expect(typeof invocation.command).toBe("string");
  });

  it("folds a malicious argument into the command string with metachars escaped", () => {
    const malicious = "in & del /q C:\\* & echo";
    const invocation = buildWindowsShimInvocation(shimPath, [malicious], false);
    // The single constructed command string is what would be handed to
    // `spawnSync(command, [], { shell: true })`. It must contain the
    // malicious text only in ESCAPED form — a bare, unescaped `&` would be
    // interpreted by cmd.exe as a command separator.
    expect(invocation.command).not.toMatch(/(?<!\^)&/);
    expect(invocation.command).toContain("^&");
  });

  it("escapes metacharacters in an absolute path argument (e.g. containing a caret or ampersand)", () => {
    const trickyPath = "C:\\Users\\dev & evil\\out.wasm";
    const invocation = buildWindowsShimInvocation(shimPath, [trickyPath], false);
    expect(invocation.command).not.toMatch(/(?<!\^)&/);
  });

  it("escapes a double-quote-embedding argument so it cannot break out of its own quoting", () => {
    const invocation = buildWindowsShimInvocation(shimPath, ['normal" & injected'], false);
    // No unescaped `"` may appear except the quoting `"` pairs themselves —
    // check specifically that the metachar layer still caught the `&`.
    expect(invocation.command).not.toMatch(/(?<!\^)&/);
  });

  it("honors the caller-supplied doubleEscape flag rather than deriving it from the path", () => {
    // Same path, same argv — only the explicit `doubleEscape` flag differs.
    // Location-based guessing is gone: the flag alone decides.
    const single = buildWindowsShimInvocation(shimPath, ["a&b"], false);
    const doubled = buildWindowsShimInvocation(shimPath, ["a&b"], true);
    expect(single.command).not.toBe(doubled.command);
    expect(single.command).not.toMatch(/(?<!\^)&/);
    expect(doubled.command).not.toMatch(/(?<!\^)&/);
  });
});

describe("resolveInvocation — dispatch between shim-safe and direct spawn", () => {
  it("dispatches a POSIX path directly with shell: false and an untouched args array", () => {
    const invocation = resolveInvocation("/usr/local/bin/wasm-opt", ["-Os", "in.wasm"]);
    expect(invocation).toEqual({
      command: "/usr/local/bin/wasm-opt",
      args: ["-Os", "in.wasm"],
      shell: false,
    });
  });

  it("dispatches through buildWindowsShimInvocation when the platform is win32 and the path is a .cmd shim", async () => {
    const originalPlatform = process.platform;
    Object.defineProperty(process, "platform", { value: "win32", configurable: true });
    try {
      expect(isWindowsShim("C:\\proj\\node_modules\\.bin\\wasm-opt.cmd")).toBe(true);
      const invocation = resolveInvocation(
        "C:\\proj\\node_modules\\.bin\\wasm-opt.cmd",
        ["evil & injected"],
        true,
      );
      expect(invocation.shell).toBe(true);
      expect(invocation.args).toEqual([]);
      expect(invocation.command).not.toMatch(/(?<!\^)&/);
    } finally {
      Object.defineProperty(process, "platform", { value: originalPlatform, configurable: true });
    }
  });
});

// --- shimRelaysArgsViaPercentStar — structural double-escape detection -----

describe("shimRelaysArgsViaPercentStar", () => {
  it("detects a %*-relaying shim body (the real pnpm/npm .cmd shape)", () => {
    const shimText = [
      '@IF EXIST "%~dp0\\node.exe" (',
      '  "%~dp0\\node.exe"  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
      ") ELSE (",
      '  node  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
      ")",
    ].join("\n");
    expect(shimRelaysArgsViaPercentStar(shimText)).toBe(true);
  });

  it("does NOT flag a shim that never relays via a bare %*", () => {
    const shimText = ["@ECHO OFF", 'SET "ARG1=%1"', "node entry.js %ARG1%"].join("\n");
    expect(shimRelaysArgsViaPercentStar(shimText)).toBe(false);
  });

  it("does not false-positive on a literal %%* (an escaped/doubled percent, not an argv relay)", () => {
    expect(shimRelaysArgsViaPercentStar("echo %%*")).toBe(false);
  });

  // --- Finding 3 (round 5): a %* mention inside a comment must not count --

  it("does NOT flag a %* mention inside a REM comment line whose real invocation uses %1 individually", () => {
    const shimText = [
      "@ECHO OFF",
      "REM do not relay via %*",
      'SET "ARG1=%~1"',
      "node entry.js %ARG1%",
    ].join("\n");
    expect(shimRelaysArgsViaPercentStar(shimText)).toBe(false);
  });

  it("does NOT flag a %* mention inside an @rem comment line (leading @, the echo-suppress idiom)", () => {
    const shimText = ["@rem uses %* only in this comment", "node entry.js %1"].join("\n");
    expect(shimRelaysArgsViaPercentStar(shimText)).toBe(false);
  });

  it("does NOT flag a %* mention inside a :: comment line", () => {
    const shimText = [":: relay note: %* not used here", "node entry.js %1"].join("\n");
    expect(shimRelaysArgsViaPercentStar(shimText)).toBe(false);
  });

  it("still detects a real %* relay even when a comment ALSO mentions %* elsewhere", () => {
    const shimText = [
      "REM this shim relays via %*",
      '"%~dp0\\node.exe"  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
    ].join("\n");
    expect(shimRelaysArgsViaPercentStar(shimText)).toBe(true);
  });

  // --- Finding 2 (round 6): `@` may be followed by whitespace before `rem`,
  // and `rem` is a comment only as a whole TOKEN, not any word starting
  // with "rem" (a `\b` word boundary also fires before a hyphen).

  it("does NOT flag a %* mention inside an `@ REM ...` comment — `@` followed by whitespace before REM is legal cmd.exe syntax", () => {
    const shimText = ["@ REM note %*", "node entry.js %1"].join("\n");
    expect(shimRelaysArgsViaPercentStar(shimText)).toBe(false);
  });

  it('DOES flag a real %* relay from a live command whose name merely STARTS WITH "rem" (e.g. `rem-wrapper`) — not a comment', () => {
    const shimText = ["rem-wrapper %*", "node entry.js %2"].join("\n");
    expect(shimRelaysArgsViaPercentStar(shimText)).toBe(true);
  });

  // Finding 2, round 7: Microsoft's own documented `REM` syntax allows a `/`
  // separator too (e.g. `Rem/||(` is a documented valid REM construct), not
  // just whitespace/`.`/`:`/end-of-line — a `REM/ note %*` comment line was
  // being misclassified as a live relay before this case was added.
  it("does NOT flag a %* mention inside a `REM/ ...` comment — `/` is a documented valid separator after the REM keyword", () => {
    const shimText = ["REM/ note %*", "node entry.js %1"].join("\n");
    expect(shimRelaysArgsViaPercentStar(shimText)).toBe(false);
  });

  // All three cases together, run through the SAME corrected regex, so a fix
  // for one can't silently regress either of the others.
  it("classifies REM/, @ REM, and rem-wrapper correctly together", () => {
    expect(shimRelaysArgsViaPercentStar(["REM/ note %*", "node entry.js %1"].join("\n"))).toBe(
      false,
    );
    expect(shimRelaysArgsViaPercentStar(["@ REM note %*", "node entry.js %1"].join("\n"))).toBe(
      false,
    );
    expect(shimRelaysArgsViaPercentStar(["rem-wrapper %*", "node entry.js %2"].join("\n"))).toBe(
      true,
    );
  });
});

describe("determineDoubleEscape — direct coverage against real fixture shims (Finding 3 gap)", () => {
  let tmpDir;
  let originalPlatform;

  afterEach(() => {
    if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = undefined;
    fsExtraExisting.clear();
    if (originalPlatform !== undefined) {
      Object.defineProperty(process, "platform", { value: originalPlatform, configurable: true });
      originalPlatform = undefined;
    }
  });

  // Registers the EXACT win32-resolved target string production code would
  // compute (via the real `extractShimTargetPath`) into the `existsSync`
  // allowlist above — see the top-of-file comment on `fsExtraExisting` for
  // why a real POSIX `existsSync` can never independently confirm it.
  function makeWindowsFixture(shimBody, shimExt) {
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const binDir = path.join(tmpDir, "node_modules", ".bin");
    const pkgDir = path.join(tmpDir, "node_modules", "binaryen", "bin");
    mkdirSync(binDir, { recursive: true });
    mkdirSync(pkgDir, { recursive: true });
    const shimPath = path.join(binDir, `wasm-opt${shimExt}`);
    writeFileSync(shimPath, shimBody);
    writeFileSync(path.join(pkgDir, "wasm-opt"), "fake-binaryen-implementation");
    const targetPath = extractShimTargetPath(shimBody, path.win32.dirname(shimPath));
    fsExtraExisting.add(targetPath);
    return { shimPath, targetPath };
  }

  function mockWin32() {
    originalPlatform = process.platform;
    Object.defineProperty(process, "platform", { value: "win32", configurable: true });
  }

  it("returns true for a real .cmd shim that relays via %*", () => {
    mockWin32();
    const { shimPath } = makeWindowsFixture(
      [
        '@IF EXIST "%~dp0\\node.exe" (',
        '  "%~dp0\\node.exe"  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
        ") ELSE (",
        '  node  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
        ")",
      ].join("\n"),
      ".cmd",
    );
    expect(determineDoubleEscape(shimPath)).toBe(true);
  });

  it("returns true for a real .bat shim that relays via %* — not just .cmd", () => {
    mockWin32();
    const { shimPath } = makeWindowsFixture(
      ["@ECHO OFF", '"%~dp0\\node.exe"  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*'].join("\n"),
      ".bat",
    );
    expect(determineDoubleEscape(shimPath)).toBe(true);
  });

  it("returns false for a .cmd shim whose only %* mention is inside a REM comment — the Finding-3 regression case", () => {
    mockWin32();
    const { shimPath } = makeWindowsFixture(
      [
        "@ECHO OFF",
        "REM do not relay via %*",
        '"%~dp0\\node.exe"  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %1 %2 %3',
      ].join("\n"),
      ".cmd",
    );
    expect(determineDoubleEscape(shimPath)).toBe(false);
  });

  it("returns false off Windows regardless of shim content", () => {
    expect(determineDoubleEscape("/usr/local/bin/wasm-opt")).toBe(false);
  });
});

// --- Finding 2: optimizer identity reaches the digest -----------------------

describe("computeCacheDigest", () => {
  const base = {
    inputBytes: Buffer.from("fake-wasm-bytes"),
    wasmOptVersion: "wasm-opt version 130 (version_130)",
    wasmOptArgs: ["-Os"],
  };

  it("produces DIFFERENT digests for two optimizer binaries reporting the SAME version string", () => {
    const digestA = computeCacheDigest({ ...base, optimizerContentHash: "hash-of-binary-a" });
    const digestB = computeCacheDigest({ ...base, optimizerContentHash: "hash-of-binary-b" });
    // This is the core Finding 2 regression: two DIFFERENT underlying
    // optimizer binaries (e.g. a patched fork) that happen to report an
    // identical `--version` string must not collide onto the same cache key.
    expect(digestA).not.toBe(digestB);
  });

  it("is deterministic given the same content hash", () => {
    const digest1 = computeCacheDigest({ ...base, optimizerContentHash: "stable-hash" });
    const digest2 = computeCacheDigest({ ...base, optimizerContentHash: "stable-hash" });
    expect(digest1).toBe(digest2);
  });

  it("changes when the input bytes change (content-addressing sanity check)", () => {
    const digestA = computeCacheDigest({ ...base, optimizerContentHash: "h" });
    const digestB = computeCacheDigest({
      ...base,
      inputBytes: Buffer.from("different-wasm-bytes"),
      optimizerContentHash: "h",
    });
    expect(digestA).not.toBe(digestB);
  });

  it("changes when wasm-opt args change", () => {
    const digestA = computeCacheDigest({ ...base, optimizerContentHash: "h" });
    const digestB = computeCacheDigest({
      ...base,
      optimizerContentHash: "h",
      wasmOptArgs: ["-O3"],
    });
    expect(digestA).not.toBe(digestB);
  });

  it("throws rather than silently caching under an unresolved-identity placeholder", () => {
    expect(() => computeCacheDigest({ ...base, optimizerContentHash: null })).toThrow(
      /requires a resolved optimizerContentHash/,
    );
    expect(() => computeCacheDigest({ ...base, optimizerContentHash: undefined })).toThrow();
    expect(() => computeCacheDigest({ ...base })).toThrow();
  });
});

describe("extractShimTargetPath — pnpm/npm .cmd shim parsing", () => {
  const shimDir = "C:\\proj\\node_modules\\.bin";

  it("resolves the node.exe-launched target through a %~dp0-relative path", () => {
    const shimText = [
      '@IF EXIST "%~dp0\\node.exe" (',
      '  "%~dp0\\node.exe"  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
      ") ELSE (",
      '  node  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
      ")",
    ].join("\n");
    const resolved = extractShimTargetPath(shimText, shimDir);
    expect(resolved).toBe(path.win32.resolve("C:\\proj\\node_modules\\binaryen\\bin\\wasm-opt"));
  });

  it("resolves a %_prog%-launched target (the ELSE-branch node fallback shape)", () => {
    const shimText =
      'endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\\..\\binaryen\\bin\\wasm-opt" %*';
    const resolved = extractShimTargetPath(shimText, shimDir);
    expect(resolved).toBe(path.win32.resolve("C:\\proj\\node_modules\\binaryen\\bin\\wasm-opt"));
  });

  it("throws when the shim shape is unrecognized rather than silently returning the shim itself", () => {
    expect(() => extractShimTargetPath("this is not a shim at all", shimDir)).toThrow(
      /could not locate the real optimizer target/,
    );
  });
});

describe("extractPosixShimTargetPath — pnpm/npm POSIX shell shim parsing", () => {
  const shimDir = "/proj/node_modules/.bin";

  it("resolves the $basedir/node-launched target (the real pnpm shim shape verified in this checkout)", () => {
    const shimText = [
      "#!/bin/sh",
      'basedir=$(dirname "$(echo "$0" | sed -e \'s,\\\\,/,g\')")',
      "",
      'if [ -x "$basedir/node" ]; then',
      '  exec "$basedir/node"  "$basedir/../binaryen/bin/wasm-opt" "$@"',
      "else",
      '  exec node  "$basedir/../binaryen/bin/wasm-opt" "$@"',
      "fi",
    ].join("\n");
    const resolved = extractPosixShimTargetPath(shimText, shimDir);
    expect(resolved).toBe(path.resolve("/proj/node_modules/binaryen/bin/wasm-opt"));
  });

  it("resolves the bare-node ELSE-branch target too", () => {
    const shimText = 'exec node  "$basedir/../binaryen/bin/wasm-opt" "$@"';
    const resolved = extractPosixShimTargetPath(shimText, shimDir);
    expect(resolved).toBe(path.resolve("/proj/node_modules/binaryen/bin/wasm-opt"));
  });

  it("throws when the shim shape is unrecognized rather than silently returning the shim itself", () => {
    expect(() => extractPosixShimTargetPath("this is not a shim at all", shimDir)).toThrow(
      /could not locate the real optimizer target/,
    );
  });
});

describe("isPosixShellShim — structural detection, not name/extension-based", () => {
  let tmpDir;

  afterEach(() => {
    if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = undefined;
  });

  it("returns true for a #!/bin/sh-shebang file", () => {
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const shimPath = path.join(tmpDir, "wasm-opt");
    writeFileSync(shimPath, '#!/bin/sh\nexec node "$basedir/../binaryen/bin/wasm-opt" "$@"\n');
    expect(isPosixShellShim(shimPath)).toBe(true);
  });

  it("returns false for a binary-shaped file with no shebang (does not read the whole file)", () => {
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const binPath = path.join(tmpDir, "wasm-opt");
    // ELF-magic-shaped header, not a shebang.
    writeFileSync(binPath, Buffer.from([0x7f, 0x45, 0x4c, 0x46, 0, 0, 0, 0]));
    expect(isPosixShellShim(binPath)).toBe(false);
  });

  it("returns false for a non-existent path rather than throwing", () => {
    expect(isPosixShellShim("/does/not/exist/wasm-opt")).toBe(false);
  });
});

// --- End-to-end Finding-2 regression: real POSIX shim shape, real digest ---
//
// This is the test the finding specifically requires: it must exercise the
// REAL `resolveActualOptimizerFile` → `hashFile` → `computeCacheDigest` path
// against a shim-shaped fixture on disk — a synthetic hash passed in
// directly (as `computeCacheDigest`'s own unit tests do above) could not
// have caught the pre-fix bug, because the pre-fix bug was in resolution,
// not digest computation.
describe("resolveActualOptimizerFile → hashFile → computeCacheDigest (POSIX shim, end to end)", () => {
  let tmpDir;

  afterEach(() => {
    if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = undefined;
  });

  function makeFixture() {
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const binDir = path.join(tmpDir, "node_modules", ".bin");
    const pkgDir = path.join(tmpDir, "node_modules", "binaryen", "bin");
    mkdirSync(binDir, { recursive: true });
    mkdirSync(pkgDir, { recursive: true });

    // The REAL pnpm-generated shape verified against this checkout's own
    // `packages/wasm/node_modules/.bin/wasm-opt`.
    const shimPath = path.join(binDir, "wasm-opt");
    writeFileSync(
      shimPath,
      [
        "#!/bin/sh",
        'basedir=$(dirname "$(echo "$0" | sed -e \'s,\\\\,/,g\')")',
        "",
        'if [ -x "$basedir/node" ]; then',
        '  exec "$basedir/node"  "$basedir/../binaryen/bin/wasm-opt" "$@"',
        "else",
        '  exec node  "$basedir/../binaryen/bin/wasm-opt" "$@"',
        "fi",
      ].join("\n"),
      { mode: 0o755 },
    );

    const targetPath = path.join(pkgDir, "wasm-opt");
    return { shimPath, targetPath };
  }

  it("resolves through the shim to the real target and hashes the TARGET's bytes, not the shim's", () => {
    const { shimPath, targetPath } = makeFixture();
    writeFileSync(targetPath, "fake-binaryen-implementation-v1");

    const resolved = resolveActualOptimizerFile(shimPath);
    expect(resolved).toBe(path.resolve(targetPath));
    expect(hashFile(resolved)).toBe(hashFile(targetPath));
    expect(hashFile(resolved)).not.toBe(hashFile(shimPath));
  });

  it("PROVES the pre-fix bug: two different optimizer binaries behind an IDENTICAL shim wrapper now produce DIFFERENT digests", () => {
    const fixtureA = makeFixture();
    writeFileSync(fixtureA.targetPath, "fake-binaryen-implementation-v1");
    const hashA = hashFile(resolveActualOptimizerFile(fixtureA.shimPath));
    const shimBytesA = hashFile(fixtureA.shimPath);
    rmSync(tmpDir, { recursive: true, force: true });

    const fixtureB = makeFixture();
    writeFileSync(fixtureB.targetPath, "fake-binaryen-implementation-v2-DIFFERENT");
    const hashB = hashFile(resolveActualOptimizerFile(fixtureB.shimPath));
    const shimBytesB = hashFile(fixtureB.shimPath);

    // The two shim WRAPPERS are byte-identical (same fixture text) — this is
    // exactly the pre-fix collision condition: pre-fix, `resolveActualOptimizerFile`
    // returned the shim path itself unchanged on POSIX, so `hashFile` hashed
    // the identical wrapper bytes for both, and these two hashes would have
    // been EQUAL despite genuinely different underlying optimizers.
    expect(shimBytesA).toBe(shimBytesB);

    // Post-fix: resolution reaches through to the differing target content.
    expect(hashA).not.toBe(hashB);

    const digestA = computeCacheDigest({
      inputBytes: Buffer.from("same-input-wasm-bytes"),
      wasmOptVersion: "wasm-opt version 130 (version_130)",
      optimizerContentHash: hashA,
      wasmOptArgs: ["-Os"],
    });
    const digestB = computeCacheDigest({
      inputBytes: Buffer.from("same-input-wasm-bytes"),
      wasmOptVersion: "wasm-opt version 130 (version_130)",
      optimizerContentHash: hashB,
      wasmOptArgs: ["-Os"],
    });
    expect(digestA).not.toBe(digestB);

    rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = undefined;
  });

  it("throws loudly when the resolved shim target does not exist on disk", () => {
    const { shimPath } = makeFixture();
    // targetPath deliberately never written.
    expect(() => resolveActualOptimizerFile(shimPath)).toThrow(/does not exist/);
  });
});

// --- Finding 1 (round 5): single-read combined identity resolution ---------
//
// `resolveActualOptimizerFile` and `determineDoubleEscape` each independently
// call `inspectWindowsShim`, which rereads the shim file from disk — a
// caller needing both answers (main()'s only such caller) used to call them
// separately, reading the shim TWICE and risking a TOCTOU divergence between
// the hashed target and the actually-invoked target/escape decision.
// `resolveOptimizerIdentity` is the single-call replacement; these tests
// prove it returns the SAME answers the two-call pattern would have, from
// one shim body.
describe("resolveOptimizerIdentity — combined single-read resolution (Finding 1)", () => {
  let tmpDir;
  let originalPlatform;

  afterEach(() => {
    if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = undefined;
    fsExtraExisting.clear();
    readFileSyncCalls.length = 0;
    if (originalPlatform !== undefined) {
      Object.defineProperty(process, "platform", { value: originalPlatform, configurable: true });
      originalPlatform = undefined;
    }
  });

  it("matches resolveActualOptimizerFile + determineDoubleEscape for a Windows %*-relaying .cmd shim", () => {
    originalPlatform = process.platform;
    Object.defineProperty(process, "platform", { value: "win32", configurable: true });

    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const binDir = path.join(tmpDir, "node_modules", ".bin");
    const pkgDir = path.join(tmpDir, "node_modules", "binaryen", "bin");
    mkdirSync(binDir, { recursive: true });
    mkdirSync(pkgDir, { recursive: true });
    const shimPath = path.join(binDir, "wasm-opt.cmd");
    const shimBody = [
      '@IF EXIST "%~dp0\\node.exe" (',
      '  "%~dp0\\node.exe"  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
      ") ELSE (",
      '  node  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
      ")",
    ].join("\n");
    writeFileSync(shimPath, shimBody);
    writeFileSync(path.join(pkgDir, "wasm-opt"), "fake-binaryen-implementation");
    // See the top-of-file comment on `fsExtraExisting`: a real POSIX
    // `existsSync` can never confirm a win32-resolved (backslash) target
    // string against this host's actual forward-slash fixture file.
    fsExtraExisting.add(extractShimTargetPath(shimBody, path.win32.dirname(shimPath)));

    const combined = resolveOptimizerIdentity(shimPath);
    expect(combined.targetPath).toBe(resolveActualOptimizerFile(shimPath));
    expect(combined.needsDoubleEscape).toBe(determineDoubleEscape(shimPath));
    expect(combined.needsDoubleEscape).toBe(true);
  });

  // --- Finding 3 (round 6): genuinely prove single-read, not just shape ------
  //
  // The two tests above only prove `resolveOptimizerIdentity` RETURNS the
  // same answers a two-call (`resolveActualOptimizerFile` +
  // `determineDoubleEscape`) pattern would — an implementation that
  // internally reintroduced that exact two-call pattern (reading the shim
  // TWICE, reopening the TOCTOU window) would still pass them. This test
  // instead counts the actual `readFileSync` calls made against the shim
  // path during ONE `resolveOptimizerIdentity` invocation and asserts
  // exactly one.
  it("reads the shim file from disk exactly ONCE per call — proves no internal double-read", () => {
    originalPlatform = process.platform;
    Object.defineProperty(process, "platform", { value: "win32", configurable: true });

    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const binDir = path.join(tmpDir, "node_modules", ".bin");
    const pkgDir = path.join(tmpDir, "node_modules", "binaryen", "bin");
    mkdirSync(binDir, { recursive: true });
    mkdirSync(pkgDir, { recursive: true });
    const shimPath = path.join(binDir, "wasm-opt.cmd");
    const shimBody = [
      '@IF EXIST "%~dp0\\node.exe" (',
      '  "%~dp0\\node.exe"  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
      ") ELSE (",
      '  node  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*',
      ")",
    ].join("\n");
    writeFileSync(shimPath, shimBody);
    writeFileSync(path.join(pkgDir, "wasm-opt"), "fake-binaryen-implementation");
    fsExtraExisting.add(extractShimTargetPath(shimBody, path.win32.dirname(shimPath)));

    readFileSyncCalls.length = 0;
    resolveOptimizerIdentity(shimPath);
    expect(readFileSyncCalls.filter((p) => p === shimPath)).toHaveLength(1);
  });

  it("matches resolveActualOptimizerFile + determineDoubleEscape for a POSIX shim (needsDoubleEscape always false)", () => {
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const binDir = path.join(tmpDir, "node_modules", ".bin");
    const pkgDir = path.join(tmpDir, "node_modules", "binaryen", "bin");
    mkdirSync(binDir, { recursive: true });
    mkdirSync(pkgDir, { recursive: true });
    const shimPath = path.join(binDir, "wasm-opt");
    writeFileSync(
      shimPath,
      [
        "#!/bin/sh",
        'basedir=$(dirname "$(echo "$0" | sed -e \'s,\\\\,/,g\')")',
        "",
        'if [ -x "$basedir/node" ]; then',
        '  exec "$basedir/node"  "$basedir/../binaryen/bin/wasm-opt" "$@"',
        "else",
        '  exec node  "$basedir/../binaryen/bin/wasm-opt" "$@"',
        "fi",
      ].join("\n"),
      { mode: 0o755 },
    );
    writeFileSync(path.join(pkgDir, "wasm-opt"), "fake-binaryen-implementation");

    const combined = resolveOptimizerIdentity(shimPath);
    expect(combined.targetPath).toBe(resolveActualOptimizerFile(shimPath));
    expect(combined.needsDoubleEscape).toBe(false);
  });

  it("passes a plain (non-shim) executable through unchanged with needsDoubleEscape false", () => {
    expect(resolveOptimizerIdentity("/usr/local/bin/wasm-opt")).toEqual({
      targetPath: "/usr/local/bin/wasm-opt",
      needsDoubleEscape: false,
    });
  });
});

// --- Finding 2 (round 5): the uncached fallback must be atomic too ---------
//
// `spawnOptimizerToTempOutput` is the shared helper both the ordinary
// cache-miss path and the "could not resolve wasm-opt's own binary identity"
// uncached fallback now route through, so a crash/interrupt never leaves a
// torn file at the requested output path even when nothing gets cached.
// These tests spawn REAL subprocesses (a tiny inline Node script standing in
// for `wasm-opt`) rather than mocking `spawnSync`, so they exercise the
// actual temp-file-write / cleanup-on-failure behavior.
describe("spawnOptimizerToTempOutput — atomic temp-file pattern (Finding 2)", () => {
  let tmpDir;

  afterEach(() => {
    if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = undefined;
  });

  it("writes the optimizer's real output to tmpOutput and reports ok:true on success", () => {
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const tmpOutput = path.join(tmpDir, ".wasm-opt-cache.123.tmp");
    const result = spawnOptimizerToTempOutput({
      command: process.execPath,
      args: [
        "-e",
        'require("node:fs").writeFileSync(process.argv[1], "optimized-bytes")',
        "--",
        tmpOutput,
      ],
      tmpOutput,
    });
    expect(result).toEqual({ ok: true });
    expect(existsSync(tmpOutput)).toBe(true);
    expect(readFileSync(tmpOutput, "utf8")).toBe("optimized-bytes");
  });

  it("removes a TORN tmpOutput the optimizer partially wrote before failing — no torn file survives", () => {
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const tmpOutput = path.join(tmpDir, ".wasm-opt-cache.456.tmp");
    const result = spawnOptimizerToTempOutput({
      command: process.execPath,
      args: [
        "-e",
        'require("node:fs").writeFileSync(process.argv[1], "TORN-partial-bytes"); process.exit(1)',
        "--",
        tmpOutput,
      ],
      tmpOutput,
    });
    expect(result.ok).toBe(false);
    expect(result.status).toBe(1);
    expect(existsSync(tmpOutput)).toBe(false);
  });

  it("removes a stale leftover tmpOutput and reports failure when the command cannot be spawned at all", () => {
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const tmpOutput = path.join(tmpDir, ".wasm-opt-cache.789.tmp");
    writeFileSync(tmpOutput, "stale-leftover-from-a-prior-run");
    const result = spawnOptimizerToTempOutput({
      command: path.join(tmpDir, "definitely-does-not-exist-wasm-opt"),
      args: ["-Os"],
      tmpOutput,
    });
    expect(result.ok).toBe(false);
    expect(existsSync(tmpOutput)).toBe(false);
  });
});

// --- runUncachedFallback — main()'s uncached-fallback branch, end to end ---
// (Finding 3, round 6)
//
// The prior `spawnOptimizerToTempOutput` tests above only exercise that
// helper in isolation — they do not prove `main()`'s actual "could not
// resolve wasm-opt's own binary identity" branch routes through it. `main()`
// itself parses `process.argv` and calls `process.exit`, so it is not
// practically drivable in-process from a test; the uncached-fallback body was
// extracted into the exported `runUncachedFallback` (mirroring how
// `spawnOptimizerToTempOutput` was already extracted) specifically so this
// branch has a directly-testable, real-subprocess-exercising entry point.
// These tests spawn a REAL fake `wasm-opt` found via `PATH` (exactly how the
// production code resolves it in this branch — a bare `"wasm-opt"` command
// under `shell: false`), and spy on `renameSync` to prove the temp-file
// write happens at a path DISTINCT from `outputPath` and is only THEN
// renamed into place — an implementation that wrote directly to `outputPath`
// (skipping the temp file + atomic rename) would call `renameSync` zero
// times here and fail the assertions below.
describe("runUncachedFallback — main()'s uncached-fallback branch, exercised end-to-end (Finding 3, round 6)", () => {
  let tmpDir;
  let fakeBinDir;
  let originalPath;

  afterEach(() => {
    if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = undefined;
    if (fakeBinDir) rmSync(fakeBinDir, { recursive: true, force: true });
    fakeBinDir = undefined;
    if (originalPath !== undefined) {
      process.env.PATH = originalPath;
      originalPath = undefined;
    }
    renameSyncCalls.length = 0;
  });

  // Installs a real, executable fake `wasm-opt` on `PATH` that writes
  // `content` to whatever path follows its `-o` argument — standing in for
  // the real optimizer without needing binaryen installed.
  function installFakeWasmOpt({ content = "uncached-optimized-bytes", exitCode = 0 } = {}) {
    fakeBinDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-fakebin-"));
    const fakeWasmOpt = path.join(fakeBinDir, "wasm-opt");
    writeFileSync(
      fakeWasmOpt,
      [
        "#!/usr/bin/env node",
        "const fs = require('node:fs');",
        "const argv = process.argv.slice(2);",
        "const outIdx = argv.indexOf('-o');",
        exitCode === 0
          ? "fs.writeFileSync(argv[outIdx + 1], " + JSON.stringify(content) + ");"
          : "",
        `process.exit(${exitCode});`,
      ].join("\n"),
      { mode: 0o755 },
    );
    originalPath = process.env.PATH;
    process.env.PATH = `${fakeBinDir}${path.delimiter}${originalPath}`;
  }

  it("writes through a distinct temp file and atomically renames it into outputPath — never a direct write", () => {
    installFakeWasmOpt({ content: "uncached-optimized-bytes" });
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const inputPath = path.join(tmpDir, "in.wasm");
    const outputPath = path.join(tmpDir, "out.wasm");
    writeFileSync(inputPath, "input-bytes");

    renameSyncCalls.length = 0;
    runUncachedFallback({ wasmOptArgs: ["-Os"], inputPath, outputPath });

    // The actual output is correct...
    expect(existsSync(outputPath)).toBe(true);
    expect(readFileSync(outputPath, "utf8")).toBe("uncached-optimized-bytes");

    // ...and it got there via EXACTLY one rename from a temp path distinct
    // from `outputPath` — proving the temp-file+rename invariant was
    // actually exercised, not bypassed by a direct write.
    expect(renameSyncCalls).toHaveLength(1);
    const [oldPath, newPath] = renameSyncCalls[0];
    expect(newPath).toBe(outputPath);
    expect(oldPath).not.toBe(outputPath);
    expect(path.basename(oldPath)).toMatch(/^\.wasm-opt-cache\.\d+\.tmp$/);
    // The temp file itself must not survive past the rename.
    expect(existsSync(oldPath)).toBe(false);
  });

  it("never touches .cache/wasm-opt — this branch has no resolved identity to key a cache entry on", () => {
    installFakeWasmOpt({ content: "uncached-optimized-bytes" });
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const inputPath = path.join(tmpDir, "in.wasm");
    const outputPath = path.join(tmpDir, "out.wasm");
    writeFileSync(inputPath, "input-bytes");

    runUncachedFallback({ wasmOptArgs: ["-Os"], inputPath, outputPath });

    expect(existsSync(path.join(tmpDir, ".cache", "wasm-opt"))).toBe(false);
  });

  it("propagates failure and calls process.exit with the fallback's exit status when the run fails — no output written", () => {
    installFakeWasmOpt({ exitCode: 1 });
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const inputPath = path.join(tmpDir, "in.wasm");
    const outputPath = path.join(tmpDir, "out.wasm");
    writeFileSync(inputPath, "input-bytes");

    const exitError = new Error("process.exit called");
    const exitSpy = vi.spyOn(process, "exit").mockImplementation(() => {
      throw exitError;
    });
    renameSyncCalls.length = 0;
    try {
      expect(() => runUncachedFallback({ wasmOptArgs: ["-Os"], inputPath, outputPath })).toThrow(
        exitError,
      );
      expect(exitSpy).toHaveBeenCalledWith(1);
    } finally {
      exitSpy.mockRestore();
    }
    expect(existsSync(outputPath)).toBe(false);
    expect(renameSyncCalls).toHaveLength(0);
  });
});

// --- finalizeCacheMissOutput — cache-write-time consistency check ---------
// (Finding 1, round 6)
//
// `resolveOptimizerIdentity` reads the shim ONCE to compute the cache-key
// identity, but the actual `wasm-opt` invocation always re-resolves through
// `resolvedWasmOptPath` — not the already-resolved `optimizerFile` — so if
// the shim/target changes between that identity read and the run completing,
// the cache entry could bind a digest computed from pre-run state to output
// reflecting post-run behavior. `finalizeCacheMissOutput` closes this with a
// cache-write-time consistency re-hash rather than full locking.
describe("finalizeCacheMissOutput — cache-write-time consistency check (Finding 1, round 6)", () => {
  let tmpDir;

  afterEach(() => {
    if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = undefined;
    copyFileSyncFailFor.clear();
    renameSyncFailFor.clear();
    rmSyncFailFor.clear();
  });

  function makeFixture() {
    tmpDir = mkdtempSync(path.join(os.tmpdir(), "wasm-opt-cache-test-"));
    const cacheDir = path.join(tmpDir, ".cache", "wasm-opt");
    mkdirSync(cacheDir, { recursive: true });
    const optimizerFile = path.join(tmpDir, "optimizer-binary");
    const tmpOutput = path.join(tmpDir, ".wasm-opt-cache.123.tmp");
    const outputPath = path.join(tmpDir, "out.wasm");
    writeFileSync(tmpOutput, "freshly-optimized-bytes");
    return { cacheDir, optimizerFile, tmpOutput, outputPath };
  }

  it("populates the cache and the output when the optimizer binary is unchanged (the normal case)", () => {
    const { cacheDir, optimizerFile, tmpOutput, outputPath } = makeFixture();
    writeFileSync(optimizerFile, "stable-optimizer-content");
    const optimizerContentHash = hashFile(optimizerFile);
    const digest = "deadbeef";
    const cachedEntryPath = path.join(cacheDir, `${digest}.wasm`);
    const tmpCacheEntry = path.join(cacheDir, `.wasm-opt-cache.${digest}.123.tmp`);

    finalizeCacheMissOutput({
      tmpOutput,
      tmpCacheEntry,
      cachedEntryPath,
      outputPath,
      optimizerFile,
      optimizerContentHash,
      resolvedWasmOptPath: optimizerFile,
    });

    expect(existsSync(cachedEntryPath)).toBe(true);
    expect(readFileSync(cachedEntryPath, "utf8")).toBe("freshly-optimized-bytes");
    expect(existsSync(outputPath)).toBe(true);
    expect(readFileSync(outputPath, "utf8")).toBe("freshly-optimized-bytes");
    expect(existsSync(tmpCacheEntry)).toBe(false);
    expect(existsSync(tmpOutput)).toBe(false);
  });

  it("skips cache population but still populates outputPath when the optimizer binary changed mid-run", () => {
    const { cacheDir, optimizerFile, tmpOutput, outputPath } = makeFixture();
    // Simulate the identity read having observed a DIFFERENT (earlier) state
    // of the optimizer binary than what's on disk by the time this runs.
    writeFileSync(optimizerFile, "PRE-run-optimizer-content");
    const staleOptimizerContentHash = hashFile(optimizerFile);
    writeFileSync(optimizerFile, "POST-run-optimizer-content-DIFFERENT");

    const digest = "deadbeef";
    const cachedEntryPath = path.join(cacheDir, `${digest}.wasm`);
    const tmpCacheEntry = path.join(cacheDir, `.wasm-opt-cache.${digest}.123.tmp`);

    const stderrSpy = vi.spyOn(process.stderr, "write").mockImplementation(() => true);
    try {
      finalizeCacheMissOutput({
        tmpOutput,
        tmpCacheEntry,
        cachedEntryPath,
        outputPath,
        optimizerFile,
        optimizerContentHash: staleOptimizerContentHash,
        resolvedWasmOptPath: optimizerFile,
      });
      expect(stderrSpy).toHaveBeenCalledWith(
        expect.stringMatching(/changed between identity resolution/),
      );
    } finally {
      stderrSpy.mockRestore();
    }

    // The cache entry must NOT be written — it would be keyed on stale identity.
    expect(existsSync(cachedEntryPath)).toBe(false);
    expect(existsSync(tmpCacheEntry)).toBe(false);
    // But the actual work product is still correct and usable.
    expect(existsSync(outputPath)).toBe(true);
    expect(readFileSync(outputPath, "utf8")).toBe("freshly-optimized-bytes");
    expect(existsSync(tmpOutput)).toBe(false);
  });

  // Round-6's re-hash-and-compare check only detects the resolved TARGET's
  // own content changing — it re-hashes the SAME `optimizerFile` path that
  // was resolved before the run. It cannot see the shim being RETARGETED to
  // point at a DIFFERENT file while the original target stays byte-identical:
  // `main()` always invokes wasm-opt through `resolvedWasmOptPath` (the
  // original, still-mutable shim path), never through the already-resolved
  // target, so a shim rewritten mid-build to point elsewhere would actually
  // run the NEW target while round-6's check keeps comparing the OLD target's
  // (unchanged) hash and would wrongly conclude nothing changed.
  it("skips cache population but still populates outputPath when the shim is RETARGETED to a different file (not just its own content changing) (Finding 1, round 7)", () => {
    const { cacheDir, tmpOutput, outputPath } = makeFixture();

    const binDir = path.join(tmpDir, "node_modules", ".bin");
    const pkgDirA = path.join(tmpDir, "node_modules", "binaryen-a", "bin");
    const pkgDirB = path.join(tmpDir, "node_modules", "binaryen-b", "bin");
    mkdirSync(binDir, { recursive: true });
    mkdirSync(pkgDirA, { recursive: true });
    mkdirSync(pkgDirB, { recursive: true });

    const shimPath = path.join(binDir, "wasm-opt");
    const targetA = path.join(pkgDirA, "wasm-opt");
    const targetB = path.join(pkgDirB, "wasm-opt");
    writeFileSync(targetA, "REAL-optimizer-A-content");
    writeFileSync(targetB, "COMPLETELY-DIFFERENT-optimizer-B-content");

    function shimBodyFor(target) {
      return [
        "#!/bin/sh",
        'basedir=$(dirname "$(echo "$0" | sed -e \'s,\\\\,/,g\')")',
        "",
        'if [ -x "$basedir/node" ]; then',
        `  exec "$basedir/node"  "${target}" "$@"`,
        "else",
        `  exec node  "${target}" "$@"`,
        "fi",
      ].join("\n");
    }

    // The shim resolves to target A at identity-resolution time.
    writeFileSync(shimPath, shimBodyFor(targetA), { mode: 0o755 });
    const { targetPath: optimizerFile } = resolveOptimizerIdentity(shimPath);
    expect(optimizerFile).toBe(path.resolve(targetA));
    const optimizerContentHash = hashFile(optimizerFile);

    // Between identity resolution and the run completing, the shim is
    // REWRITTEN to point at a completely different target (B) — target A's
    // own bytes never change.
    writeFileSync(shimPath, shimBodyFor(targetB), { mode: 0o755 });
    expect(hashFile(targetA)).toBe(optimizerContentHash);

    const digest = "deadbeef-retarget";
    const cachedEntryPath = path.join(cacheDir, `${digest}.wasm`);
    const tmpCacheEntry = path.join(cacheDir, `.wasm-opt-cache.${digest}.123.tmp`);

    const stderrSpy = vi.spyOn(process.stderr, "write").mockImplementation(() => true);
    try {
      finalizeCacheMissOutput({
        tmpOutput,
        tmpCacheEntry,
        cachedEntryPath,
        outputPath,
        optimizerFile,
        optimizerContentHash,
        resolvedWasmOptPath: shimPath,
      });
      expect(stderrSpy).toHaveBeenCalledWith(
        expect.stringMatching(/shim's resolved target.*changed between identity resolution/),
      );
    } finally {
      stderrSpy.mockRestore();
    }

    // The cache entry must NOT be written — the shim now resolves to B, a
    // file whose content was never hashed for this cache key.
    expect(existsSync(cachedEntryPath)).toBe(false);
    expect(existsSync(tmpCacheEntry)).toBe(false);
    // But the actual work product is still correct and usable.
    expect(existsSync(outputPath)).toBe(true);
    expect(readFileSync(outputPath, "utf8")).toBe("freshly-optimized-bytes");
    expect(existsSync(tmpOutput)).toBe(false);
  });

  // Finding 3, round 7: a failure INSIDE the post-run consistency check
  // itself (e.g. the previously resolved target vanished from disk) must
  // never propagate out and discard an otherwise-successful run — it must be
  // treated exactly like "identity could not be confirmed" (skip the cache
  // write) and still satisfy `outputPath`.
  it("skips cache population but still populates outputPath, and does not throw, when the post-run identity re-confirmation itself fails", () => {
    const { cacheDir, optimizerFile, tmpOutput, outputPath } = makeFixture();
    writeFileSync(optimizerFile, "stable-optimizer-content");
    const optimizerContentHash = hashFile(optimizerFile);

    // Simulate the previously resolved target having vanished/been replaced
    // by the time the post-run check runs (e.g. ENOENT re-hashing it).
    rmSync(optimizerFile, { force: true });

    const digest = "deadbeef-vanished";
    const cachedEntryPath = path.join(cacheDir, `${digest}.wasm`);
    const tmpCacheEntry = path.join(cacheDir, `.wasm-opt-cache.${digest}.123.tmp`);

    const stderrSpy = vi.spyOn(process.stderr, "write").mockImplementation(() => true);
    try {
      expect(() =>
        finalizeCacheMissOutput({
          tmpOutput,
          tmpCacheEntry,
          cachedEntryPath,
          outputPath,
          optimizerFile,
          optimizerContentHash,
          resolvedWasmOptPath: optimizerFile,
        }),
      ).not.toThrow();
      expect(stderrSpy).toHaveBeenCalledWith(
        expect.stringMatching(/could not re-confirm the optimizer's identity/),
      );
    } finally {
      stderrSpy.mockRestore();
    }

    expect(existsSync(cachedEntryPath)).toBe(false);
    expect(existsSync(tmpCacheEntry)).toBe(false);
    // The actual work product must still be delivered — a failure to
    // CONFIRM identity is never a reason to fail the whole build.
    expect(existsSync(outputPath)).toBe(true);
    expect(readFileSync(outputPath, "utf8")).toBe("freshly-optimized-bytes");
    expect(existsSync(tmpOutput)).toBe(false);
  });

  // Finding 1, round 8: identity IS confirmed (the run genuinely succeeded)
  // but persisting the result to the cache fails (disk full, permissions,
  // EIO, ...). This is a CACHE-side failure only — it must never discard a
  // correct, successful build output. Covers both cache-population calls:
  // `copyFileSync(tmpOutput, tmpCacheEntry)` and the subsequent
  // `renameSync(tmpCacheEntry, cachedEntryPath)`.
  it("skips cache population but still populates outputPath, and does not throw, when copyFileSync into the cache fails", () => {
    const { cacheDir, optimizerFile, tmpOutput, outputPath } = makeFixture();
    writeFileSync(optimizerFile, "stable-optimizer-content");
    const optimizerContentHash = hashFile(optimizerFile);

    const digest = "deadbeef-copyfail";
    const cachedEntryPath = path.join(cacheDir, `${digest}.wasm`);
    const tmpCacheEntry = path.join(cacheDir, `.wasm-opt-cache.${digest}.123.tmp`);
    copyFileSyncFailFor.add(tmpCacheEntry);

    const stderrSpy = vi.spyOn(process.stderr, "write").mockImplementation(() => true);
    try {
      expect(() =>
        finalizeCacheMissOutput({
          tmpOutput,
          tmpCacheEntry,
          cachedEntryPath,
          outputPath,
          optimizerFile,
          optimizerContentHash,
          resolvedWasmOptPath: optimizerFile,
        }),
      ).not.toThrow();
      expect(stderrSpy).toHaveBeenCalledWith(
        expect.stringMatching(/confirmed the optimizer's identity but failed to persist/),
      );
    } finally {
      stderrSpy.mockRestore();
    }

    // The cache entry must NOT exist — the copy into it failed.
    expect(existsSync(cachedEntryPath)).toBe(false);
    expect(existsSync(tmpCacheEntry)).toBe(false);
    // But the actual work product — a genuinely successful run — is still
    // delivered. A cache-side failure is never a build failure.
    expect(existsSync(outputPath)).toBe(true);
    expect(readFileSync(outputPath, "utf8")).toBe("freshly-optimized-bytes");
    expect(existsSync(tmpOutput)).toBe(false);
  });

  it("skips cache population but still populates outputPath, and does not throw, when the final cache renameSync fails", () => {
    const { cacheDir, optimizerFile, tmpOutput, outputPath } = makeFixture();
    writeFileSync(optimizerFile, "stable-optimizer-content");
    const optimizerContentHash = hashFile(optimizerFile);

    const digest = "deadbeef-renamefail";
    const cachedEntryPath = path.join(cacheDir, `${digest}.wasm`);
    const tmpCacheEntry = path.join(cacheDir, `.wasm-opt-cache.${digest}.123.tmp`);
    renameSyncFailFor.add(cachedEntryPath);

    const stderrSpy = vi.spyOn(process.stderr, "write").mockImplementation(() => true);
    try {
      expect(() =>
        finalizeCacheMissOutput({
          tmpOutput,
          tmpCacheEntry,
          cachedEntryPath,
          outputPath,
          optimizerFile,
          optimizerContentHash,
          resolvedWasmOptPath: optimizerFile,
        }),
      ).not.toThrow();
      expect(stderrSpy).toHaveBeenCalledWith(
        expect.stringMatching(/confirmed the optimizer's identity but failed to persist/),
      );
    } finally {
      stderrSpy.mockRestore();
    }

    // The cache entry must NOT exist — the rename into it failed — and the
    // partial cache temp file must be cleaned up rather than left behind.
    expect(existsSync(cachedEntryPath)).toBe(false);
    expect(existsSync(tmpCacheEntry)).toBe(false);
    // But the actual work product — a genuinely successful run — is still
    // delivered. A cache-side failure is never a build failure.
    expect(existsSync(outputPath)).toBe(true);
    expect(readFileSync(outputPath, "utf8")).toBe("freshly-optimized-bytes");
    expect(existsSync(tmpOutput)).toBe(false);
  });

  // Round 9: the cache-population failure handler's own cleanup
  // (`rmSync(tmpCacheEntry, { force: true })`) can ITSELF throw a genuine I/O
  // error — `force: true` only suppresses "already doesn't exist", not
  // EIO/EPERM/etc on removal. If that exception is left to propagate, it
  // bypasses the final `renameSync(tmpOutput, outputPath)` below it,
  // reintroducing exactly the bug this whole handler exists to close: a real,
  // successful build's output gets stranded in `tmpOutput` instead of
  // delivered to `outputPath`. Cleanup must be best-effort.
  it("still populates outputPath when BOTH the cache rename AND the cleanup of the partial cache temp file fail", () => {
    const { cacheDir, optimizerFile, tmpOutput, outputPath } = makeFixture();
    writeFileSync(optimizerFile, "stable-optimizer-content");
    const optimizerContentHash = hashFile(optimizerFile);

    const digest = "deadbeef-renamefail-and-cleanupfail";
    const cachedEntryPath = path.join(cacheDir, `${digest}.wasm`);
    const tmpCacheEntry = path.join(cacheDir, `.wasm-opt-cache.${digest}.123.tmp`);
    renameSyncFailFor.add(cachedEntryPath);
    rmSyncFailFor.add(tmpCacheEntry);

    const stderrSpy = vi.spyOn(process.stderr, "write").mockImplementation(() => true);
    try {
      expect(() =>
        finalizeCacheMissOutput({
          tmpOutput,
          tmpCacheEntry,
          cachedEntryPath,
          outputPath,
          optimizerFile,
          optimizerContentHash,
          resolvedWasmOptPath: optimizerFile,
        }),
      ).not.toThrow();
      expect(stderrSpy).toHaveBeenCalledWith(
        expect.stringMatching(/confirmed the optimizer's identity but failed to persist/),
      );
    } finally {
      stderrSpy.mockRestore();
    }

    // The cache entry must NOT exist — the rename into it failed.
    expect(existsSync(cachedEntryPath)).toBe(false);
    // The partial cache temp file is left behind — its own cleanup failed —
    // but that must be a harmless, recoverable artifact, never a build
    // failure. The line below is documentation of that tolerated state, not
    // an assertion this test depends on for its pass/fail verdict.
    expect(existsSync(tmpCacheEntry)).toBe(true);
    // The actual work product — a genuinely successful run — is still
    // delivered despite BOTH failures. This is the assertion that fails
    // against the pre-fix tree (the propagating rmSync throw skips the final
    // renameSync entirely, leaving outputPath unpopulated and tmpOutput
    // stranded) and passes against the fix.
    expect(existsSync(outputPath)).toBe(true);
    expect(readFileSync(outputPath, "utf8")).toBe("freshly-optimized-bytes");
    expect(existsSync(tmpOutput)).toBe(false);
  });
});

// --- Finding 4 (round 5): literal (non-special) shimDir substitution -------
//
// `.replace(regex, someString)` treats `$&`/`$$`/`` $` ``/`$'`/`$1`-`$9`
// specially INSIDE the replacement string argument. Both shim-target
// resolvers pass the shim's own directory as that replacement — an absolute
// filesystem path that happens to contain one of those sequences must still
// be inserted literally, not reinterpreted as a backreference/match token.
describe("literal $&/$1-safe shimDir substitution (Finding 4)", () => {
  it("POSIX: a shimDir containing a $&-shaped substring is inserted literally, not as 'matched text'", () => {
    const shimDir = "/proj/weird $& dir/node_modules/.bin";
    const shimText = 'exec node  "$basedir/../binaryen/bin/wasm-opt" "$@"';
    const resolved = extractPosixShimTargetPath(shimText, shimDir);
    expect(resolved).toBe(path.resolve(`${shimDir}/../binaryen/bin/wasm-opt`));
  });

  it("POSIX: a shimDir containing a $1-shaped substring is inserted literally", () => {
    const shimDir = "/proj/$1-backup/node_modules/.bin";
    const shimText = 'exec node  "$basedir/../binaryen/bin/wasm-opt" "$@"';
    const resolved = extractPosixShimTargetPath(shimText, shimDir);
    expect(resolved).toBe(path.resolve(`${shimDir}/../binaryen/bin/wasm-opt`));
  });

  it("Windows: a shimDir containing a $&-shaped substring is inserted literally", () => {
    const shimDir = "C:\\proj\\weird $& dir\\node_modules\\.bin";
    const shimText = '"%~dp0\\node.exe"  "%~dp0\\..\\binaryen\\bin\\wasm-opt" %*';
    const resolved = extractShimTargetPath(shimText, shimDir);
    expect(resolved).toBe(path.win32.resolve(`${shimDir}\\..\\binaryen\\bin\\wasm-opt`));
  });
});
