// Regression coverage for the Linux libc fail-closed fix in
// `./rustHostTriple.ts` (mirrored in
// `packages/dx-harness/src/core/rustHostTriple.ts`): an inconclusive
// `detectLinuxLibc()` result must make `hostRustTriples` offer the gnu
// triple ONLY, never both gnu and musl — an open ambiguity there let a
// downstream "pick the newest candidate" search select an
// ABI-incompatible cross-build purely because it had a newer mtime.

import { afterEach, describe, expect, it, vi } from "vitest";

import { hostRustTriples } from "./rustHostTriple.js";

describe("hostRustTriples — Linux libc fail-closed selection (real host, unmocked)", () => {
  it("offers gnu ONLY (not musl) for x64 when libc detection is inconclusive on a real host", () => {
    // We can't force `detectLinuxLibc()` to return `undefined` from outside
    // without mocking fs/process.platform — this suite runs on darwin in CI,
    // so `detectLinuxLibc()` short-circuits to `undefined` for real here (a
    // genuinely inconclusive-detection run, not a simulated one: the
    // function deliberately ignores the `platform` param passed to
    // `hostRustTriples` and consults the REAL `process.platform`).
    const triples = hostRustTriples("linux", "x64");
    expect(triples).toEqual(["x86_64-unknown-linux-gnu"]);
    expect(triples).not.toContain("x86_64-unknown-linux-musl");
  });

  it("offers gnu ONLY (not musl) for arm64 when libc detection is inconclusive on a real host", () => {
    const triples = hostRustTriples("linux", "arm64");
    expect(triples).toEqual(["aarch64-unknown-linux-gnu"]);
    expect(triples).not.toContain("aarch64-unknown-linux-musl");
  });

  it("returns no candidates for an unsupported linux arch", () => {
    expect(hostRustTriples("linux", "ia32")).toEqual([]);
  });

  it("is unaffected on non-linux platforms", () => {
    expect(hostRustTriples("darwin", "arm64")).toEqual(["aarch64-apple-darwin"]);
    expect(hostRustTriples("win32", "x64")).toEqual(["x86_64-pc-windows-msvc"]);
  });
});

// --- Direct, mocked coverage of detectLinuxLibc's own branches --------------
//
// The tests above prove the fail-closed DEFAULT (real host inconclusive ⇒
// gnu-only) but can't exercise the ld-musl-marker positive-detection branch
// on a non-Linux CI box. These tests simulate `process.platform === "linux"`
// and mock `node:fs`'s `existsSync` to drive each `detectLinuxLibc` branch
// directly, importing the module fresh (`vi.resetModules`) per test so the
// mock is picked up cleanly.

const fsExistsState: { existing: Set<string> } = { existing: new Set() };

vi.mock("node:fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:fs")>();
  return {
    ...actual,
    existsSync: (p: string) => fsExistsState.existing.has(p),
  };
});

// `detectLinuxLibc`/`hostRustTriples` read `process.platform`/`process.arch`
// at CALL time, not at module-load time — so the simulated platform/arch
// must still be in effect while `fn` runs, and only restored once `fn` (and
// therefore every call it makes) has returned. Restoring eagerly right after
// the dynamic `import()` (before `fn` runs) would silently make every
// assertion observe the REAL host platform again.
//
// `detectLinuxLibc()` also reads `process.report?.getReport().header
// .glibcVersionRuntime` BEFORE ever touching the filesystem — on a real
// glibc-linked Node (this repo's own CI runners included) that branch fires
// for real and returns "gnu" immediately, short-circuiting every mocked fs
// scenario below regardless of `existingPaths`. `glibcVersionRuntime` forces
// that signal deterministically: omitted/`undefined` reproduces a
// non-glibc-reporting Node (falls through to the fs markers, the case every
// existing fs-driven test below actually wants); a string reproduces a
// glibc-linked Node and short-circuits to "gnu" before any fs check runs.
async function withLinuxHost<T>(
  arch: string,
  existingPaths: string[],
  fn: (mod: typeof import("./rustHostTriple.js")) => T,
  glibcVersionRuntime?: string,
): Promise<T> {
  fsExistsState.existing = new Set(existingPaths);
  const originalPlatform = process.platform;
  const originalArch = process.arch;
  const originalReport = process.report;
  Object.defineProperty(process, "platform", { value: "linux", configurable: true });
  Object.defineProperty(process, "arch", { value: arch, configurable: true });
  Object.defineProperty(process, "report", {
    value:
      glibcVersionRuntime === undefined
        ? undefined
        : { getReport: () => ({ header: { glibcVersionRuntime } }) },
    configurable: true,
  });
  vi.resetModules();
  try {
    const mod = await import("./rustHostTriple.js");
    return fn(mod);
  } finally {
    Object.defineProperty(process, "platform", { value: originalPlatform, configurable: true });
    Object.defineProperty(process, "arch", { value: originalArch, configurable: true });
    Object.defineProperty(process, "report", { value: originalReport, configurable: true });
  }
}

describe("detectLinuxLibc — mocked linux host, per-branch coverage", () => {
  afterEach(() => {
    fsExistsState.existing = new Set();
  });

  it("returns undefined (inconclusive) when no signal is present", async () => {
    const result = await withLinuxHost("x64", [], (mod) => mod.detectLinuxLibc());
    expect(result).toBeUndefined();
  });

  it("returns musl when the Alpine marker file is present", async () => {
    const result = await withLinuxHost("x64", ["/etc/alpine-release"], (mod) =>
      mod.detectLinuxLibc(),
    );
    expect(result).toBe("musl");
  });

  it("returns musl when the x64 ld-musl dynamic linker marker is present (non-Alpine musl)", async () => {
    const result = await withLinuxHost("x64", ["/lib/ld-musl-x86_64.so.1"], (mod) =>
      mod.detectLinuxLibc(),
    );
    expect(result).toBe("musl");
  });

  it("returns musl when the arm64 ld-musl dynamic linker marker is present (non-Alpine musl)", async () => {
    const result = await withLinuxHost("arm64", ["/lib/ld-musl-aarch64.so.1"], (mod) =>
      mod.detectLinuxLibc(),
    );
    expect(result).toBe("musl");
  });

  it("hostRustTriples offers musl-only once the ld-musl marker resolves musl positively", async () => {
    const triples = await withLinuxHost("x64", ["/lib/ld-musl-x86_64.so.1"], (mod) =>
      mod.hostRustTriples("linux", "x64"),
    );
    expect(triples).toEqual(["x86_64-unknown-linux-musl"]);
  });

  it("hostRustTriples fails closed to gnu-only when no signal resolves (inconclusive)", async () => {
    const triples = await withLinuxHost("x64", [], (mod) => mod.hostRustTriples("linux", "x64"));
    expect(triples).toEqual(["x86_64-unknown-linux-gnu"]);
    expect(triples).not.toContain("x86_64-unknown-linux-musl");
  });

  it("returns gnu immediately when process.report reports a glibc version, without consulting fs markers", async () => {
    const result = await withLinuxHost(
      "x64",
      ["/lib/ld-musl-x86_64.so.1"], // a musl fs marker present too — glibc must still win, checked first
      (mod) => mod.detectLinuxLibc(),
      "2.35",
    );
    expect(result).toBe("gnu");
  });

  it("hostRustTriples offers gnu-only when process.report reports a glibc version", async () => {
    const triples = await withLinuxHost(
      "x64",
      [],
      (mod) => mod.hostRustTriples("linux", "x64"),
      "2.35",
    );
    expect(triples).toEqual(["x86_64-unknown-linux-gnu"]);
    expect(triples).not.toContain("x86_64-unknown-linux-musl");
  });
});
