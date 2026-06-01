/**
 * Per-platform fallback behavior of the REAL NAPI-generated `dist/index.js`
 * loader, proven hermetically with `node:vm` (no installer, no network, no
 * `.node`). This REPLACES the old `loaderSource.includes("@verter/native-…")`
 * substring guard in `pack-shape.spec.ts`: a substring match proves a name
 * appears SOMEWHERE in the source; it does NOT prove the loader actually
 * requires that exact package for that platform, nor that musl/gnu and arch
 * selection are wired correctly. This spec runs the loader's own bytes under
 * a faked `process` and asserts WHAT it requires.
 *
 * For each canonical platform (derived from `package.json#napi.targets` via
 * `platforms.ts`, NOT from `optionalDependencies`) we present that platform's
 * `process.platform`/`arch`/musl signal, force the local `dist/*.node` branch
 * to miss (empty dist), satisfy ONLY the published optional-dependency
 * packages, and assert the loader RESOLVED and ACCEPTED exactly the published
 * `@verter/native-<triple>` for that platform and nothing else published. We
 * assert ACCEPTANCE (the loader exported a sentinel AND did NOT throw
 *— `accepted` / `acceptedSentinelPackage`), not mere attempt nor merely
 * that require returned the sentinel: a loader that requested (or even
 * returned) the right id but then threw must NOT count. The
 * darwin branch ATTEMPTS the intentionally-unpublished
 * `@verter/native-darwin-universal` alias first; that attempt is an
 * explicitly-allowed non-resolution (it is not in `optionalDependencies`), so
 * "exactly" means the published platform package, not the attempt list.
 *
 * This covers the Yarn-PnP / npm / pnpm concern at the MECHANISM level: it
 * proves which optional dependency the loader declares it needs; every
 * package manager that honors a declared optional dependency then resolves
 * it the same way. It is independent of any particular installer layout.
 */

import { describe, expect, it } from "vitest";
import { PLATFORM_MATRIX, type PlatformEntry } from "./platforms.ts";
import {
  probeLoaderForPlatform,
  readGeneratedLoaderSource,
  readPublishedOptionalDeps,
  type ProbeFakePlatform,
} from "./test-helpers/loader-probe.ts";

const loaderSource = readGeneratedLoaderSource();
const published = readPublishedOptionalDeps();

/** Map a canonical matrix entry to the fake platform signal to present. */
function fakeFor(entry: PlatformEntry): ProbeFakePlatform {
  return {
    platform: entry.os,
    arch: entry.cpu,
    // Only linux distinguishes musl vs gnu; drive it from the matrix libc.
    musl: entry.os === "linux" ? entry.libc === "musl" : undefined,
  };
}

describe("issue #90 — generated loader per-platform optional-dependency fallback (VM)", () => {
  it("has a non-trivial canonical matrix to exercise", () => {
    // Guards against a vacuous pass if the matrix ever derives empty.
    expect(PLATFORM_MATRIX.length).toBe(7);
  });

  it.each(PLATFORM_MATRIX.map((e) => [e.napiTriple, e] as const))(
    "loader for %s resolves exactly the published platform package @verter/native-<triple> via the optional-dependency fallback",
    (_triple, entry) => {
      const result = probeLoaderForPlatform(loaderSource, fakeFor(entry), published);

      // It ACCEPTED our sentinel as the final binding (exported AND did not
      // throw) — not merely attempted the right id, and not export-then-throw.
      expect(result.threwMessage).toBeNull();
      expect(result.accepted).toBe(true);
      // The package the loader ACCEPTED (read back from the exported
      // sentinel) is exactly this platform's package.
      expect(result.acceptedSentinelPackage).toBe(entry.packageName);

      // It ACCEPTED EXACTLY the right published package — no more, no less.
      // This is the no-throw-gated accepted set, so neither an unpublished
      // alias attempt that throws nor an export-then-throw id appears here.
      expect(result.acceptedPublishedDepRequests).toEqual([entry.packageName]);

      // It never accepted a published package for a DIFFERENT platform.
      const wrongPublished = result.acceptedPublishedDepRequests.filter(
        (id) => id !== entry.packageName,
      );
      expect(wrongPublished).toEqual([]);

      // "Exactly the published platform package": the darwin branch ATTEMPTS
      // the intentionally-unpublished `@verter/native-darwin-universal`
      // alias FIRST (it is not in optionalDependencies), which throws
      // MODULE_NOT_FOUND and falls through. That attempt is therefore an
      // EXPLICITLY-ALLOWED non-resolution — assert it was attempted (on
      // darwin) yet never resolved, so "exactly" refers to the published
      // resolution, not the attempt list.
      const DARWIN_UNIVERSAL_ALIAS = "@verter/native-darwin-universal";
      if (entry.os === "darwin") {
        expect(result.attemptedDepRequests).toContain(DARWIN_UNIVERSAL_ALIAS);
      }
      expect(result.acceptedPublishedDepRequests).not.toContain(DARWIN_UNIVERSAL_ALIAS);
    },
  );

  it("linux x64 selects gnu vs musl strictly from the musl signal (no cross-leak)", () => {
    const gnu = probeLoaderForPlatform(
      loaderSource,
      { platform: "linux", arch: "x64", musl: false },
      published,
    );
    const musl = probeLoaderForPlatform(
      loaderSource,
      { platform: "linux", arch: "x64", musl: true },
      published,
    );
    // Each side ACCEPTED cleanly (exported AND did not throw).
    expect(gnu.threwMessage).toBeNull();
    expect(musl.threwMessage).toBeNull();
    expect(gnu.accepted).toBe(true);
    expect(musl.accepted).toBe(true);
    expect(gnu.acceptedPublishedDepRequests).toEqual(["@verter/native-linux-x64-gnu"]);
    expect(musl.acceptedPublishedDepRequests).toEqual(["@verter/native-linux-x64-musl"]);
    // The loader ACCEPTED (not just attempted) the right package each way.
    expect(gnu.acceptedSentinelPackage).toBe("@verter/native-linux-x64-gnu");
    expect(musl.acceptedSentinelPackage).toBe("@verter/native-linux-x64-musl");
    // The two are genuinely different packages — proves the signal flips it.
    expect(gnu.acceptedPublishedDepRequests).not.toEqual(musl.acceptedPublishedDepRequests);
  });

  it("linux arm64 selects gnu vs musl strictly from the musl signal", () => {
    const gnu = probeLoaderForPlatform(
      loaderSource,
      { platform: "linux", arch: "arm64", musl: false },
      published,
    );
    const musl = probeLoaderForPlatform(
      loaderSource,
      { platform: "linux", arch: "arm64", musl: true },
      published,
    );
    // Each side ACCEPTED cleanly (exported AND did not throw).
    expect(gnu.threwMessage).toBeNull();
    expect(musl.threwMessage).toBeNull();
    expect(gnu.accepted).toBe(true);
    expect(musl.accepted).toBe(true);
    expect(gnu.acceptedPublishedDepRequests).toEqual(["@verter/native-linux-arm64-gnu"]);
    expect(musl.acceptedPublishedDepRequests).toEqual(["@verter/native-linux-arm64-musl"]);
    // The loader ACCEPTED (not just attempted) the right package each way.
    expect(gnu.acceptedSentinelPackage).toBe("@verter/native-linux-arm64-gnu");
    expect(musl.acceptedSentinelPackage).toBe("@verter/native-linux-arm64-musl");
  });

  // ---- Discrimination self-proof -----------------------------------------
  // The per-triple assertion above is `toEqual([entry.packageName])`. To
  // prove that assertion is not vacuous, run the SAME probe but compare
  // against a deliberately WRONG expected package: it must NOT match. If the
  // probe returned a constant (a stub), both the right and wrong comparisons
  // would behave identically and this guard would fail.
  it("a WRONG expected triple does not match (discrimination guard)", () => {
    const linuxX64 = PLATFORM_MATRIX.find((e) => e.napiTriple === "linux-x64-gnu")!;
    const result = probeLoaderForPlatform(loaderSource, fakeFor(linuxX64), published);

    // It ACCEPTED cleanly (exported AND did not throw).
    expect(result.threwMessage).toBeNull();
    expect(result.accepted).toBe(true);
    // Right package matches.
    expect(result.acceptedPublishedDepRequests).toEqual(["@verter/native-linux-x64-gnu"]);
    // A different platform's package does NOT match — the assertion bites.
    expect(result.acceptedPublishedDepRequests).not.toEqual(["@verter/native-win32-x64-msvc"]);
    expect(result.acceptedPublishedDepRequests).not.toEqual(["@verter/native-linux-x64-musl"]);
  });

  // The probe must surface a clear failure when the matching published
  // package is ABSENT — proving the resolution actually depends on the
  // package existing (not a constant). This is the missing-optional-dep
  // case at the loader level; item 8 asserts the surfaced message shape.
  it("with the matching published dep removed, the loader does NOT resolve a sentinel", () => {
    const win = PLATFORM_MATRIX.find((e) => e.napiTriple === "win32-x64-msvc")!;
    const withoutWin = new Set(
      [...published].filter((name) => name !== "@verter/native-win32-x64-msvc"),
    );
    const result = probeLoaderForPlatform(loaderSource, fakeFor(win), withoutWin);
    expect(result.accepted).toBe(false);
    expect(result.acceptedSentinelPackage).toBeNull();
    expect(result.acceptedPublishedDepRequests).toEqual([]);
    // It threw the napi "cannot find native binding" terminal error.
    expect(result.threwMessage).toMatch(/native binding/i);
  });

  // ---- Accepted-not-attempted discrimination (issue #90 round-2 item 3) ----
  // Prove the per-triple assertions are checking ACCEPTANCE, not merely that
  // the right id was ATTEMPTED. Construct the case where the loader STILL
  // requests the right package (it is win32-x64-msvc) but that package is NOT
  // in the published set, so the require throws and the loader falls through:
  //   - the loader DID attempt @verter/native-win32-x64-msvc (it appears in
  //     `attemptedDepRequests`), yet
  //   - it did NOT accept it (`acceptedPublishedDepRequests` is empty,
  //     `accepted` is false, `acceptedSentinelPackage` is null).
  // An assertion built on `attemptedDepRequests` would PASS here (the id was
  // attempted) — which is the bug. The accepted-based assertions FAIL here,
  // which is correct. This is the executable proof the new fields bite.
  it("attempted-but-unaccepted: the right package is ATTEMPTED yet not ACCEPTED when its require throws", () => {
    const win = PLATFORM_MATRIX.find((e) => e.napiTriple === "win32-x64-msvc")!;
    const withoutWin = new Set([...published].filter((name) => name !== win.packageName));
    const result = probeLoaderForPlatform(loaderSource, fakeFor(win), withoutWin);

    // The loader DID request the right package (attempted) ...
    expect(result.attemptedDepRequests).toContain(win.packageName);
    // ... but it was NOT accepted. The accepted-based signals all say "no",
    // even though the id is in the attempted list. THIS is what makes the
    // per-triple `acceptedSentinelPackage === packageName` assertion above a
    // real acceptance check rather than an attempted-membership check. It also
    // threw (the package was absent), so the no-throw gate independently
    // confirms non-acceptance.
    expect(result.acceptedPublishedDepRequests).toEqual([]);
    expect(result.accepted).toBe(false);
    expect(result.acceptedSentinelPackage).toBeNull();
    expect(result.threwMessage).not.toBeNull();
  });

  // ---- Export-then-throw gate (issue #90 round-3 finding 2) ---------------
  // The ACCEPTED fields must gate on no-throw. A loader that RETURNS the
  // sentinel from `require`, ASSIGNS it to `module.exports`, and THEN throws
  // (a post-export version check, or any export-then-throw shape) must NOT be
  // reported as accepted — even though `module.exports.__PROBE_SENTINEL__` is
  // set. We craft exactly that loader source (the probe runs whatever bytes it
  // is handed) and prove:
  //   - it DID return the sentinel from require (`returnedByRequireDepRequests`
  //     contains the id) — so the OLD probe's `resolvedPublishedDepRequests`
  //     (the un-gated require-returned set) would have been `[pkg]`, and its
  //     `resolvedSentinel` (exported-symbol check, no no-throw gate) would have
  //     been `true` — i.e. it would have FALSELY passed every success-path
  //     assertion;
  //   - the throw IS recorded (`threwMessage` non-null);
  //   - the NEW ACCEPTED fields are all false/null/empty (the gate bites).
  // This is the executable fail-before/pass-after proof for the gate.
  it("export-then-throw: a loader that exports the sentinel then throws is NOT accepted (no-throw gate)", () => {
    const pkg = "@verter/native-win32-x64-msvc";
    // A minimal loader that resolves the published sentinel, exports it, and
    // then throws — the export-then-throw shape the gate must reject. It uses
    // the SAME global `Symbol.for(...)` the probe checks, via the sentinel the
    // intercepted require hands back, so `module.exports.__PROBE_SENTINEL__`
    // genuinely equals the probe's SENTINEL when the throw fires.
    // `module.exports = binding` carries BOTH `__PROBE_SENTINEL__` and
    // `requestedId` (the intercepted require returns them together), so the
    // probe sees a fully-exported sentinel right before the throw.
    const exportThenThrowSource = [
      `const binding = require(${JSON.stringify(pkg)});`,
      `module.exports = binding;`,
      `throw new Error('post-export version check failed (export-then-throw)');`,
    ].join("\n");

    const result = probeLoaderForPlatform(
      exportThenThrowSource,
      { platform: "win32", arch: "x64" },
      published,
    );

    // The require DID return the sentinel for the package (diagnostic survives
    // the throw). This is precisely what the OLD un-gated
    // `resolvedPublishedDepRequests` reported — proving the bug was reachable.
    expect(result.returnedByRequireDepRequests).toEqual([pkg]);
    // The loader threw AFTER exporting — the throw is recorded.
    expect(result.threwMessage).not.toBeNull();
    expect(result.threwMessage!).toMatch(/export-then-throw/);

    // The no-throw gate bites: despite the sentinel reaching `module.exports`,
    // NONE of the ACCEPTED signals report success.
    expect(result.accepted).toBe(false);
    expect(result.acceptedSentinelPackage).toBeNull();
    expect(result.acceptedPublishedDepRequests).toEqual([]);

    // Discrimination self-proof: the SAME source WITHOUT the trailing throw IS
    // accepted — so the only thing flipping acceptance is the throw, i.e. the
    // gate keys on no-throw and nothing else.
    const cleanSource = [
      `const binding = require(${JSON.stringify(pkg)});`,
      `module.exports = binding;`,
    ].join("\n");
    const clean = probeLoaderForPlatform(
      cleanSource,
      { platform: "win32", arch: "x64" },
      published,
    );
    expect(clean.threwMessage).toBeNull();
    expect(clean.accepted).toBe(true);
    expect(clean.acceptedSentinelPackage).toBe(pkg);
    expect(clean.acceptedPublishedDepRequests).toEqual([pkg]);
  });
});
