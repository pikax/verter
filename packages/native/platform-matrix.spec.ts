/**
 * Single-source-of-truth reconciliation for the supported-platform set
 * (issue #90 item 3). The canonical matrix is derived in `platforms.ts`
 * from the AUTHORITATIVE `package.json#napi.targets` (the rust-target list
 * the napi build is driven by). This spec asserts the FOUR other places
 * that independently enumerate platforms all agree with it, and FAILS on
 * any mismatch — catching "a triple was dropped or added in one location
 * but not the others":
 *
 *   (a) `package.json#optionalDependencies` keys == `@verter/native-<triple>`
 *       for every matrix triple, and nothing extra.
 *   (a-spec) every `optionalDependencies` VERSION spec is the lock-step
 *       `workspace:*` (none drifted to a pinned/foreign range) — the version
 *       SPEC, not just the key. The published `npm/<triple>/package.json`
 *       `version` is locked to the main package version separately, in
 *       `platform-packages.spec.ts` (its `tpl.version === MAIN_VERSION`
 *       assertion); together they keep the platform packages in lock-step.
 *   (b) a `packages/native/npm/<triple>/` template dir exists for every
 *       triple, and there are no extra dirs.
 *   (c) the generated `dist/index.js` loader actually ACCEPTS (exports the
 *       sentinel AND does NOT throw) `@verter/native-<triple>` for every
 *       triple — proven by executing the loader per-platform in a VM (item 4
 *       mechanism), NOT a substring, and asserting ACCEPTANCE not mere
 *       attempt.
 *   (d) the `.github/workflows/release.yml` `build-native` job's target
 *       matrix == the rust-target set (parsed hermetically).
 *   (e) the set of published platform packages the loader can RESOLVE ==
 *       the canonical matrix set EXACTLY (no extras) — closing (c)'s one-way
 *       direction so a stale loader branch for a dropped triple is caught.
 *       The unpublished `@verter/native-darwin-universal` alias is an
 *       explicitly-allowed non-resolution, asserted as such (not ignored).
 *
 * Discrimination is self-proven below: deriving a VARIANT matrix from a
 * temp package.json with one target removed makes every reconciliation
 * arm report the now-orphaned location.
 */

import { describe, expect, it } from "vitest";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  PACKAGE_DIR,
  PLATFORM_MATRIX,
  buildPlatformMatrix,
  optionalDependencyPackageNames,
  type PlatformEntry,
} from "./platforms.ts";
import {
  probeLoaderForPlatform,
  readGeneratedLoaderSource,
  readPublishedOptionalDeps,
} from "./test-helpers/loader-probe.ts";
import { readBuildNativeTargets } from "./test-helpers/release-workflow.ts";

const loaderSource = readGeneratedLoaderSource();
const published = readPublishedOptionalDeps();

function readOptionalDependencies(): Record<string, string> {
  const pkg = JSON.parse(readFileSync(join(PACKAGE_DIR, "package.json"), "utf8")) as {
    optionalDependencies?: Record<string, string>;
  };
  return pkg.optionalDependencies ?? {};
}

function readOptionalDependencyKeys(): string[] {
  return Object.keys(readOptionalDependencies());
}

/** The lock-step version spec every per-platform optional dep must carry. */
const EXPECTED_OPTIONAL_DEP_SPEC = "workspace:*";

/** The `npm/<triple>` directories that actually exist on disk. */
function existingNpmTripleDirs(): string[] {
  const npmDir = join(PACKAGE_DIR, "npm");
  return readdirSync(npmDir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name);
}

/**
 * Resolve a matrix entry through the real loader and assert it ACCEPTED
 * (exported the sentinel AND did NOT throw) — not merely attempted the right
 * id, and not export-then-throw. Returns the accepted published-package list
 * so callers can compare against `[entry.packageName]`. A loader that
 * requested (or even returned) the right package but then threw fails the
 * `accepted` / `acceptedSentinelPackage` checks here (the no-throw gate), so
 * the returned list reflects real acceptance.
 */
function acceptedPackageFor(entry: PlatformEntry, pub: Set<string>): string[] {
  const result = probeLoaderForPlatform(
    loaderSource,
    {
      platform: entry.os,
      arch: entry.cpu,
      musl: entry.os === "linux" ? entry.libc === "musl" : undefined,
    },
    pub,
  );
  // Acceptance gate: it did not throw, it accepted a sentinel, and the
  // accepted package matches the accepted list. (When `pub` deliberately
  // omits the entry's package the caller is testing non-acceptance and does
  // not route through here.)
  expect(result.threwMessage, `loader threw resolving ${entry.napiTriple}`).toBeNull();
  expect(result.accepted, `loader did not accept a sentinel for ${entry.napiTriple}`).toBe(true);
  expect(result.acceptedSentinelPackage).toBe(entry.packageName);
  return result.acceptedPublishedDepRequests;
}

/**
 * Decode the `process.platform`/`arch`/musl signal that triggers the loader
 * branch for a given `@verter/native-<triple>` package NAME — for ANY napi
 * triple the loader knows, not only our canonical 7. This lets (e) probe the
 * platform behind every PUBLISHED package (including a hypothetical extra
 * outside the canonical matrix), so an extra published package the loader can
 * resolve is detectable. Returns `null` for a package whose triple this
 * decoder does not model (it intentionally covers the os/arch/libc shapes the
 * loader actually branches on).
 */
function platformSignalForPackageName(
  pkgName: string,
): { platform: NodeJS.Platform; arch: string; musl?: boolean } | null {
  const triple = pkgName.replace(/^@verter\/native-/, "");
  // win32-<arch>-msvc | win32-<arch>-gnu
  let m = /^win32-([^-]+)-(msvc|gnu)$/.exec(triple);
  if (m) return { platform: "win32", arch: m[1] };
  // darwin-<arch> (and the universal alias, which has no single arch)
  m = /^darwin-(x64|arm64)$/.exec(triple);
  if (m) return { platform: "darwin", arch: m[1] };
  // linux-<arch>-(gnu|musl|gnueabihf|musleabihf)
  m = /^linux-([^-]+)-(gnu|musl|gnueabihf|musleabihf)$/.exec(triple);
  if (m) return { platform: "linux", arch: m[1], musl: m[2].startsWith("musl") };
  // freebsd-<arch>
  m = /^freebsd-(x64|arm64)$/.exec(triple);
  if (m) return { platform: "freebsd" as NodeJS.Platform, arch: m[1] };
  return null;
}

describe("issue #90 — platform matrix reconciliation (single source of truth)", () => {
  it("derives the full 7-platform canonical matrix from package.json#napi.targets", () => {
    expect(PLATFORM_MATRIX.length).toBe(7);
    expect(new Set(PLATFORM_MATRIX.map((e) => e.napiTriple)).size).toBe(7);
  });

  it("(a) optionalDependencies keys match the canonical package names exactly", () => {
    const expected = [...optionalDependencyPackageNames()].sort();
    const actual = [...readOptionalDependencyKeys()].sort();
    expect(actual).toEqual(expected);
  });

  it("(a-spec) every optionalDependencies VERSION spec is the lock-step workspace:* (none drifted)", () => {
    const optDeps = readOptionalDependencies();
    // Per-package: each canonical platform dep is pinned to the exact
    // lock-step spec. A drift to a pinned semver or a foreign range (which
    // would make the published platform package version diverge from the
    // main package) is caught here, not just the KEY presence in (a).
    for (const name of optionalDependencyPackageNames()) {
      expect(optDeps[name], `optionalDependencies["${name}"] spec`).toBe(
        EXPECTED_OPTIONAL_DEP_SPEC,
      );
    }
    // Whole-map: build the expected map from the canonical matrix and assert
    // equality — catches BOTH a drifted spec AND an extra/foreign entry in
    // one shot.
    const expectedMap = Object.fromEntries(
      optionalDependencyPackageNames().map((name) => [name, EXPECTED_OPTIONAL_DEP_SPEC]),
    );
    expect(optDeps).toEqual(expectedMap);
    // Belt-and-braces: no spec is a pinned semver (a `1.2.3`-style value).
    for (const [name, spec] of Object.entries(optDeps)) {
      expect(spec, `optionalDependencies["${name}"] must not be a pinned semver`).not.toMatch(
        /^\d+\.\d+\.\d+/,
      );
    }
  });

  it("(b) every triple has an npm/<triple> dir, and there are no extra dirs", () => {
    const expected = [...PLATFORM_MATRIX.map((e) => e.napiTriple)].sort();
    const actual = [...existingNpmTripleDirs()].sort();
    expect(actual).toEqual(expected);
    // And each one really exists (belt-and-braces against readdir quirks).
    for (const entry of PLATFORM_MATRIX) {
      expect(existsSync(join(PACKAGE_DIR, "npm", entry.napiTriple, "package.json"))).toBe(true);
    }
  });

  it("(c) the generated loader requires @verter/native-<triple> for every triple (VM-proven)", () => {
    for (const entry of PLATFORM_MATRIX) {
      expect(
        acceptedPackageFor(entry, published),
        `loader did not accept ${entry.packageName} for ${entry.napiTriple}`,
      ).toEqual([entry.packageName]);
    }
  });

  // ---- (e) Loader resolves ONLY canonical triples (catches an EXTRA) ------
  // (c) is one-way: every CANONICAL triple resolves. (e) closes the other
  // direction — the set of PUBLISHED platform packages the generated loader
  // can RESOLVE equals the canonical matrix set EXACTLY. It is keyed off the
  // PUBLISHED set (not the canonical matrix), then asserts that resolvable set
  // == canonical: so an EXTRA published package whose loader branch fires
  // (e.g. a triple dropped from `napi.targets` and the matrix but left in
  // both optionalDependencies AND a stale generated loader) is RESOLVED here,
  // lands in the union, and breaks the union == canonical equality. The
  // package→platform decode covers every napi triple the loader branches on,
  // not just our 7, so a non-canonical extra is genuinely reachable.
  //
  // The darwin branch ATTEMPTS the intentionally-unpublished
  // `@verter/native-darwin-universal` alias FIRST; it is NOT in
  // optionalDependencies, so a real install never resolves it and it must
  // NEVER appear in the resolved set. We assert that explicitly (an allowed
  // unpublished alias, not a silent ignore) rather than letting it slip.
  it("(e) the loader resolves EXACTLY the canonical published package set (no extras)", () => {
    const DARWIN_UNIVERSAL_ALIAS = "@verter/native-darwin-universal";
    const canonical = new Set(optionalDependencyPackageNames());

    // For EVERY published package, decode the platform that fires its loader
    // branch and probe: the loader must resolve EXACTLY that published
    // package. Collect the resolvable-published union. Keying on `published`
    // (the on-disk install reality) is what makes an extra detectable.
    const resolvedUnion = new Set<string>();
    for (const pkgName of published) {
      const sig = platformSignalForPackageName(pkgName);
      expect(sig, `no platform decode for published package ${pkgName}`).not.toBeNull();
      const probe = probeLoaderForPlatform(loaderSource, sig!, published);
      // The published package's own platform ACCEPTS exactly that package
      // (exported AND did not throw).
      expect(probe.threwMessage, `loader threw accepting published ${pkgName}`).toBeNull();
      expect(
        probe.acceptedSentinelPackage,
        `loader did not accept published ${pkgName} on its own platform`,
      ).toBe(pkgName);
      expect(probe.acceptedPublishedDepRequests).toEqual([pkgName]);
      for (const r of probe.acceptedPublishedDepRequests) resolvedUnion.add(r);
      // The unpublished darwin-universal alias is NEVER accepted.
      expect(probe.acceptedPublishedDepRequests).not.toContain(DARWIN_UNIVERSAL_ALIAS);
    }

    // The resolvable-published set == the canonical set, EXACTLY. An extra
    // published package (resolved above) would make the union a superset of
    // canonical; a dropped canonical package (absent from `published`) would
    // make it a subset. Either way this equality bites.
    expect([...resolvedUnion].sort()).toEqual([...canonical].sort());

    // Explicitly account for the allowed unpublished alias: the darwin rows
    // ATTEMPT it, but it is never published and never resolved. Prove the
    // attempt was exercised (not vacuous) and that it is not canonical.
    const darwinX64 = PLATFORM_MATRIX.find((e) => e.napiTriple === "darwin-x64")!;
    const darwinProbe = probeLoaderForPlatform(
      loaderSource,
      { platform: darwinX64.os, arch: darwinX64.cpu },
      published,
    );
    expect(darwinProbe.attemptedDepRequests).toContain(DARWIN_UNIVERSAL_ALIAS);
    expect(darwinProbe.acceptedPublishedDepRequests).not.toContain(DARWIN_UNIVERSAL_ALIAS);
    expect(canonical.has(DARWIN_UNIVERSAL_ALIAS)).toBe(false);
  });

  it("(d) release.yml build-native target matrix matches the canonical rust-target set", () => {
    const expected = [...PLATFORM_MATRIX.map((e) => e.rustTarget)].sort();
    const actual = [...readBuildNativeTargets()].sort();
    expect(actual).toEqual(expected);
  });

  // ---- Discrimination self-proof -----------------------------------------
  // Build a VARIANT matrix from a hypothetical package.json that DROPPED one
  // rust-target. Every reconciliation arm must then report the orphaned
  // location, proving each arm genuinely cross-checks the matrix (not a
  // tautology comparing a thing to itself).
  describe("(discrimination) dropping a triple from the source desynchronises every location", () => {
    const FULL_TARGETS = PLATFORM_MATRIX.map((e) => e.rustTarget);
    const DROPPED = "x86_64-pc-windows-msvc";
    const variantTargets = FULL_TARGETS.filter((t) => t !== DROPPED);
    const variantMatrix = buildPlatformMatrix(variantTargets);
    const variantTriples = new Set(variantMatrix.map((e) => e.napiTriple));
    const variantPackages = new Set(variantMatrix.map((e) => e.packageName));

    it("variant matrix really is smaller and lacks the dropped triple", () => {
      expect(variantMatrix.length).toBe(6);
      expect(variantTriples.has("win32-x64-msvc")).toBe(false);
    });

    it("(a-mismatch) real optionalDependencies no longer equals the variant package set", () => {
      const variantExpected = [...variantPackages].sort();
      const realActual = [...readOptionalDependencyKeys()].sort();
      // The real package.json still HAS the win32 dep ⇒ not equal to the
      // variant ⇒ a real drop would be caught.
      expect(realActual).not.toEqual(variantExpected);
      expect(realActual).toContain("@verter/native-win32-x64-msvc");
    });

    it("(b-mismatch) real npm dirs no longer equals the variant triple set", () => {
      const variantExpected = [...variantTriples].sort();
      const realActual = [...existingNpmTripleDirs()].sort();
      expect(realActual).not.toEqual(variantExpected);
      expect(realActual).toContain("win32-x64-msvc");
    });

    it("(c-mismatch) the loader still resolves the dropped triple's package (so the (c) arm would fail)", () => {
      const win = PLATFORM_MATRIX.find((e) => e.napiTriple === "win32-x64-msvc")!;
      // Against the REAL published set the loader accepts win32 — a variant
      // matrix that omitted win32 would never assert this, so the real (c)
      // arm (which DOES include win32) is the one doing the work.
      expect(acceptedPackageFor(win, published)).toEqual(["@verter/native-win32-x64-msvc"]);
    });

    it("(d-mismatch) real release.yml matrix no longer equals the variant rust-target set", () => {
      const variantExpected = [...variantTargets].sort();
      const realActual = [...readBuildNativeTargets()].sort();
      expect(realActual).not.toEqual(variantExpected);
      expect(realActual).toContain(DROPPED);
    });

    // (e-mismatch) The generated loader carries branches for MANY platforms
    // beyond our canonical 7 (napi emits the full platform matrix, e.g.
    // freebsd/android/openharmony). That is exactly why the (e) reconciliation
    // is necessary: if such a non-canonical triple were ever published (added
    // to optionalDependencies without being a real napi.target), the loader
    // would happily resolve it and ship a half-wired platform. Prove the
    // mechanism: present a NON-canonical platform (freebsd-x64) AND inject its
    // package into the published set — the loader RESOLVES it. Against the
    // REAL published set (canonical only) this same platform resolves nothing,
    // so the (e) bidirectional equality would FAIL the moment an extra leaked
    // in. This is the executable proof (e) catches a loader extra.
    it("(e-mismatch) the loader resolves a NON-canonical published triple if one is injected (so (e) bites)", () => {
      const FREEBSD = "@verter/native-freebsd-x64";
      // The loader source genuinely has a freebsd-x64 branch ...
      expect(loaderSource).toContain(FREEBSD);
      // ... and freebsd-x64 is NOT in our canonical published set.
      expect(new Set(optionalDependencyPackageNames()).has(FREEBSD)).toBe(false);

      // With ONLY canonical packages published, freebsd resolves nothing
      // (its package is absent) — the (e) union would never include it.
      const realPublished = probeLoaderForPlatform(
        loaderSource,
        { platform: "freebsd" as NodeJS.Platform, arch: "x64" },
        published,
      );
      expect(realPublished.acceptedPublishedDepRequests).toEqual([]);
      expect(realPublished.accepted).toBe(false);

      // Inject the extra into the published set: the loader DOES accept it,
      // proving an extra leaking into the published/canonical set would be
      // caught by (e)'s bidirectional equality (the union would now contain a
      // package the canonical set does not).
      const withExtra = new Set([...published, FREEBSD]);
      const injected = probeLoaderForPlatform(
        loaderSource,
        { platform: "freebsd" as NodeJS.Platform, arch: "x64" },
        withExtra,
      );
      expect(injected.threwMessage).toBeNull();
      expect(injected.acceptedPublishedDepRequests).toEqual([FREEBSD]);
      expect(injected.accepted).toBe(true);
      expect(injected.acceptedSentinelPackage).toBe(FREEBSD);

      // End-to-end: reproduce the (e) union computation against the
      // injected-extra published set and prove it NO LONGER equals canonical
      // — i.e. the (e) assertion would FAIL with this extra. This makes "(e)
      // bites" executable, not merely "freebsd resolves in isolation".
      const canonical = new Set(optionalDependencyPackageNames());
      const unionWithExtra = new Set<string>();
      for (const pkgName of withExtra) {
        const sig = platformSignalForPackageName(pkgName);
        expect(sig, `no platform decode for ${pkgName}`).not.toBeNull();
        const probe = probeLoaderForPlatform(loaderSource, sig!, withExtra);
        for (const r of probe.acceptedPublishedDepRequests) unionWithExtra.add(r);
      }
      // The union now contains the extra and is a strict superset of canonical.
      expect(unionWithExtra.has(FREEBSD)).toBe(true);
      expect([...unionWithExtra].sort()).not.toEqual([...canonical].sort());
    });
  });
});
