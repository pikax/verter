/**
 * Hermetic diagnostics tests for the REAL generated `dist/index.js` loader
 * (issue #90 item 8): when the platform binary cannot be resolved, the
 * surfaced error must be CLEAR and actionable, not a bare MODULE_NOT_FOUND.
 *
 * Two failure modes are exercised in-process via `node:vm` (no installer,
 * no network):
 *   (a) SUPPORTED platform, matching optional-dependency ABSENT — the
 *       npm-optional-deps-bug class issue #90 was about.
 *   (b) UNSUPPORTED platform/arch — a host outside the published matrix.
 *
 * Finding (asserted below, not assumed): the napi-generated loader ALREADY
 * emits a clear, actionable TOP-LEVEL message for BOTH cases — the
 * "Cannot find native binding … optional dependencies …" guidance — and
 * carries the precise root cause on the error's `cause` chain: the raw
 * module-not-found for (a), and the "Unsupported OS/architecture" detail
 * for (b). The top-level message is therefore NEVER a bare
 * `MODULE_NOT_FOUND`. Because the loader is already clear, the thin
 * `index.js` wrapper does NOT decorate it (it must not reimplement loading
 * nor edit the generated loader). This spec PINS that property: a future
 * loader regression that started leaking a raw module-not-found at the top
 * level would fail here and force the wrapper-boundary decoration the brief
 * describes.
 */

import { describe, expect, it } from "vitest";
import { PLATFORM_MATRIX } from "./platforms.ts";
import {
  probeLoaderForPlatform,
  readGeneratedLoaderSource,
  readPublishedOptionalDeps,
} from "./test-helpers/loader-probe.ts";

const loaderSource = readGeneratedLoaderSource();
const published = readPublishedOptionalDeps();

const HOST = PLATFORM_MATRIX.find((e) => e.napiTriple === "win32-x64-msvc")!;

describe("issue #90 — generated loader diagnostics (hermetic VM)", () => {
  it("(a) supported platform with the matching optional-dep ABSENT surfaces actionable guidance, not bare MODULE_NOT_FOUND", () => {
    const result = probeLoaderForPlatform(
      loaderSource,
      { platform: HOST.os, arch: HOST.cpu },
      // No published optional deps at all ⇒ the matching package is absent.
      new Set<string>(),
    );

    expect(result.accepted).toBe(false);
    expect(result.acceptedSentinelPackage).toBeNull();
    expect(result.threwMessage).not.toBeNull();
    const msg = result.threwMessage!;

    // Top-level message is the clear napi guidance...
    expect(msg).toMatch(/Cannot find native binding/i);
    expect(msg).toMatch(/optional dependencies/i);
    // ...and is NOT itself a bare module-not-found line.
    expect(msg.startsWith("Cannot find module")).toBe(false);
    expect(msg).not.toMatch(/MODULE_NOT_FOUND/);

    // The precise root cause IS preserved on the cause chain (so debugging
    // is still possible) — it names the missing optional-dep package.
    expect(result.threwCauseChain.length).toBeGreaterThan(0);
    expect(result.threwCauseChain.some((m) => m.includes(HOST.packageName))).toBe(true);
  });

  it("(b) unsupported OS surfaces actionable top-level guidance with the platform detail on the cause chain", () => {
    // A platform not in the published matrix (sunos is a valid node platform
    // string but one we never ship a binary for).
    const result = probeLoaderForPlatform(
      loaderSource,
      { platform: "sunos" as NodeJS.Platform, arch: "x64" },
      published,
    );

    expect(result.accepted).toBe(false);
    expect(result.threwMessage).not.toBeNull();
    const msg = result.threwMessage!;
    // Clear top-level guidance, not a bare module-not-found.
    expect(msg).toMatch(/Cannot find native binding/i);
    expect(msg.startsWith("Cannot find module")).toBe(false);
    // The unsupported-platform detail is on the cause chain.
    expect(result.threwCauseChain.some((m) => /Unsupported OS:\s*sunos/i.test(m))).toBe(true);
  });

  it("(c) unsupported arch on a supported OS surfaces guidance with the arch detail on the cause chain", () => {
    const result = probeLoaderForPlatform(
      loaderSource,
      { platform: "linux", arch: "mips" },
      published,
    );
    expect(result.accepted).toBe(false);
    expect(result.threwMessage).not.toBeNull();
    expect(result.threwMessage!).toMatch(/Cannot find native binding/i);
    expect(result.threwMessage!.startsWith("Cannot find module")).toBe(false);
    expect(
      result.threwCauseChain.some((m) => /Unsupported architecture on Linux:\s*mips/i.test(m)),
    ).toBe(true);
  });

  // Discrimination: the SAME platform WITH its published dep present resolves
  // cleanly (no throw). This proves the (a) assertion is about the dep being
  // absent, not a constant "always throws".
  it("(discrimination) the same supported platform WITH its dep present resolves cleanly", () => {
    const ok = probeLoaderForPlatform(
      loaderSource,
      { platform: HOST.os, arch: HOST.cpu },
      new Set([HOST.packageName]),
    );
    expect(ok.threwMessage).toBeNull();
    expect(ok.accepted).toBe(true);
    // The loader ACCEPTED the host package as the final binding (exported and
    // did not throw, not merely attempted).
    expect(ok.acceptedSentinelPackage).toBe(HOST.packageName);
    expect(ok.threwCauseChain).toEqual([]);
  });
});
