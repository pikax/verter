import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { joinCanonical } from "../src/paths.js";
import {
  VENDORED_VUE_VERSION,
  buildVendorManifest,
  collectVuePackageVersions,
  computeExpectedVueVersion,
  sha256Hex,
  vendorShimsDir,
} from "../src/vendorManifest.js";

describe("vendored shim source", () => {
  it("exposes a canonical vendor shims dir that holds the vue shim", () => {
    const dir = vendorShimsDir();
    expect(dir).not.toContain("\\");
    expect(dir.endsWith("/vendor/shims")).toBe(true);
    // The committed vue shim is present. Join with `joinCanonical` (not bare
    // `posix.join`) so a canonical UNC `//server/share/...` base is not collapsed
    // — matching the production vendor joins in vendorManifest.ts.
    expect(() => readFileSync(joinCanonical(dir, "vue", "index.d.ts"), "utf-8")).not.toThrow();
    expect(() => readFileSync(joinCanonical(dir, "vue", "package.json"), "utf-8")).not.toThrow();
  });

  it("computes expectedVueVersion ONCE from the committed vue/package.json", () => {
    expect(computeExpectedVueVersion()).toBe(VENDORED_VUE_VERSION);
  });

  it("pins every vendored vue/@vue package to the SAME version (passes C's sync)", () => {
    const versions = collectVuePackageVersions();
    const names = versions.map((v) => v.package).sort();
    expect(names).toContain("vue");
    expect(names).toContain("@vue/compiler-core");
    expect(names).toContain("@vue/compiler-sfc");
    // Every vendored package equals the pinned line — no drift, so the bridge's
    // strict vendored-Vue version sync cannot fail on this shim set.
    for (const v of versions) {
      expect(v.version).toBe(VENDORED_VUE_VERSION);
    }
    // Negative: not a single package carries a different version.
    expect(versions.filter((v) => v.version !== VENDORED_VUE_VERSION)).toEqual([]);
  });
});

describe("buildVendorManifest", () => {
  it("checksums every committed file and is stable across builds", () => {
    const a = buildVendorManifest();
    const b = buildVendorManifest();
    expect(a).toEqual(b);
    expect(a.vueVersion).toBe(VENDORED_VUE_VERSION);
    expect(a.files.length).toBeGreaterThan(0);

    const paths = a.files.map((f) => f.path);
    expect(paths).toContain("vue/package.json");
    expect(paths).toContain("vue/index.d.ts");
    expect(paths).toContain("@vue/compiler-core/package.json");

    // Manifest entries are sorted, forward-slashed, and fully-formed.
    expect([...paths]).toEqual([...paths].sort());
    for (const f of a.files) {
      expect(f.path).not.toContain("\\");
      expect(f.bytes).toBeGreaterThan(0);
      expect(f.sha256).toMatch(/^[0-9a-f]{64}$/);
    }
  });

  it("each checksum matches the file's actual bytes on disk", () => {
    const dir = vendorShimsDir();
    const manifest = buildVendorManifest();
    const pkg = manifest.files.find((f) => f.path === "vue/package.json")!;
    const actual = sha256Hex(readFileSync(joinCanonical(dir, "vue", "package.json")));
    expect(pkg.sha256).toBe(actual);
  });
});

describe("sha256Hex", () => {
  it("is deterministic and content-sensitive (a tamper changes the digest)", () => {
    expect(sha256Hex(Buffer.from("alpha"))).toBe(sha256Hex(Buffer.from("alpha")));
    expect(sha256Hex(Buffer.from("alpha"))).not.toBe(sha256Hex(Buffer.from("alpha "))); // one byte
    // Matches the platform crypto reference.
    expect(sha256Hex(Buffer.from("alpha"))).toBe(
      createHash("sha256").update(Buffer.from("alpha")).digest("hex"),
    );
  });
});
