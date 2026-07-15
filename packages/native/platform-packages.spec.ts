/**
 * Shape guards for the per-platform optional-dependency package templates
 * (`packages/native/npm/<triple>/package.json`) — issue #90 item 5.
 *
 * The generated loader's fallback resolves `@verter/native-<triple>` and
 * then loads that package's `main` (`verter-native.<triple>.node`). If the
 * template's `name`/`main`/`files`/`os`/`cpu`/`libc`/`version` are wrong,
 * the package manager either refuses to install it on the right host
 * (wrong `os`/`cpu`/`libc`), installs it but can't find the binary (wrong
 * `main`/`files`), or installs a version-mismatched binary (drifted
 * `version`). This guard pins all of those against the canonical matrix
 * derived from `package.json#napi.targets` — so the binary the fallback
 * resolves actually loads.
 *
 * Each expected value comes from the matrix (computed from the rust-target
 * list), NOT from the template under test, so the assertion is real.
 */

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { PACKAGE_DIR, PLATFORM_MATRIX, type PlatformEntry } from "./platforms.ts";

interface PlatformPackageJson {
  name?: string;
  version?: string;
  main?: string;
  files?: string[];
  os?: string[];
  cpu?: string[];
  libc?: string[];
}

/** The main package version every platform package must match (lock-step). */
function mainPackageVersion(): string {
  const pkg = JSON.parse(readFileSync(join(PACKAGE_DIR, "package.json"), "utf8")) as {
    version: string;
  };
  return pkg.version;
}

function readTemplate(entry: PlatformEntry): PlatformPackageJson {
  const path = join(PACKAGE_DIR, "npm", entry.napiTriple, "package.json");
  return JSON.parse(readFileSync(path, "utf8")) as PlatformPackageJson;
}

const MAIN_VERSION = mainPackageVersion();

describe("issue #90 — per-platform optional-dependency package shape", () => {
  it("has the full 7-platform matrix", () => {
    expect(PLATFORM_MATRIX.length).toBe(7);
  });

  it.each(PLATFORM_MATRIX.map((e) => [e.napiTriple, e] as const))(
    "npm/%s/package.json matches the canonical matrix row",
    (_triple, entry) => {
      const tpl = readTemplate(entry);

      // Identity + entry point.
      expect(tpl.name).toBe(entry.packageName);
      expect(tpl.main).toBe(entry.nodeFileName);

      // `files` must include EXACTLY the one platform binary (it is the
      // only artifact this package ships).
      expect(tpl.files).toEqual([entry.nodeFileName]);

      // Platform constraints gate install to the right host.
      expect(tpl.os).toEqual([entry.os]);
      expect(tpl.cpu).toEqual([entry.cpu]);

      // libc is present ONLY for the linux gnu/musl split; the template tag
      // is `glibc` for gnu and `musl` for musl. darwin/win32 carry none.
      if (entry.libc) {
        expect(tpl.libc).toEqual([entry.libc]);
      } else {
        expect(tpl.libc).toBeUndefined();
      }

      // Version lock-step with the main package: a drifted platform package
      // would trip the generated loader's NAPI_RS_ENFORCE_VERSION_CHECK.
      expect(tpl.version).toBe(MAIN_VERSION);
    },
  );

  // Discrimination guard: the expected values are matrix-derived, not
  // copied from the template. Prove the row assertion bites by checking a
  // KNOWN-WRONG expectation does not hold for a real template.
  it("(discrimination) a wrong os/cpu expectation does not match a real template", () => {
    const linuxX64Gnu = PLATFORM_MATRIX.find((e) => e.napiTriple === "linux-x64-gnu")!;
    const tpl = readTemplate(linuxX64Gnu);
    // Correct row matches.
    expect(tpl.os).toEqual([linuxX64Gnu.os]);
    expect(tpl.cpu).toEqual([linuxX64Gnu.cpu]);
    expect(tpl.libc).toEqual([linuxX64Gnu.libc]);
    // Wrong rows do NOT match — the equality assertions are real.
    expect(tpl.os).not.toEqual(["win32"]);
    expect(tpl.cpu).not.toEqual(["arm64"]);
    expect(tpl.libc).not.toEqual(["musl"]);
  });
});
