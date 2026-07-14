import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { remapCarrierSpan, clearCarrierMapCache, type Manifest } from "@verter/language-shared";
import { DiskCarrierStoreReader } from "./carrierStore";

/**
 * A V3 source map with a single mapping: generated (line 1, col 0) → source
 * (line 1, col 0) in `A.vue`. `findOrigin(1, 1)` (1-based) resolves through it.
 * `AAAA` = the VLQ for `[0,0,0,0]` (gen-col 0, source 0, orig-line 0, orig-col 0).
 */
const SINGLE_MAPPING_V3 = {
  version: 3,
  sources: ["A.vue"],
  names: [],
  mappings: "AAAA",
};

function writeStore(manifest: Manifest, files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), "verter-remap-store-"));
  mkdirSync(join(dir, "blobs"), { recursive: true });
  mkdirSync(join(dir, "maps"), { recursive: true });
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(dir, rel);
    mkdirSync(join(abs, ".."), { recursive: true });
    writeFileSync(abs, content, "utf8");
  }
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest), "utf8");
  return dir;
}

function manifestWithMap(): Manifest {
  return {
    epoch: 1,
    host_version: "test",
    projects: {
      "d:/ws/tsconfig.json": {
        owned_sources: [
          {
            source_uri: "d:/ws/src/A.vue",
            provider_uri: "d:/ws/src/A.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
        ],
        ready_files: {
          "d:/ws/src/A.vue.tsx": {
            content_hash: "a1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "m1",
            blob_rel: "blobs/A.vue.tsx",
            map_rel: "maps/A.vue.json",
          },
        },
      },
    },
  };
}

let dirs: string[] = [];
beforeEach(() => {
  dirs = [];
  clearCarrierMapCache();
});
afterEach(() => {
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
});
function track(d: string): string {
  dirs.push(d);
  return d;
}

describe("remapCarrierSpan", () => {
  it("remaps a span inside the companion back to the source position via the published map", () => {
    const dir = track(
      writeStore(manifestWithMap(), {
        "blobs/A.vue.tsx": "const x = 1;\n",
        "maps/A.vue.json": JSON.stringify(SINGLE_MAPPING_V3),
      }),
    );
    const reader = new DiskCarrierStoreReader(dir);

    const remapped = remapCarrierSpan(
      reader,
      "d:/ws/src/A.vue.tsx",
      { start: 0, length: 5 },
      () => "const x = 1;\n",
      (sourcePath) => (sourcePath === "A.vue" ? "<source A.vue text>\n" : undefined),
    );

    expect(remapped).not.toBeNull();
    expect(remapped!.fileName).toBe("A.vue");
    expect(remapped!.textSpan.start).toBe(0);
    // The end offset (5) resolves within the single mapping segment's extent
    // (strict end mapping, no snap), so the source span keeps the faithful
    // length 5 — NOT a hardcoded `1`, which under-sized every span (a 5-char
    // selection collapsed to 1).
    expect(remapped!.textSpan.length).toBe(5);
  });

  it("remaps a MULTI-CHARACTER span to the correct multi-character source span via end mapping", () => {
    // Two mapping segments on gen line 1: gen-col 0 → src (line0, col2), and
    // gen-col 9 → src (line0, col8). A carrier span [start 0, length 9] maps to
    // source [start 2, length 6] — the END offset (9) resolves to source col 8,
    // so the faithful source length is 8 − 2 = 6, NOT length-1 and NOT the
    // carrier length 9. (`AAAE,SAAM`: seg1 = VLQ[0,0,0,2]; seg2 = VLQ[+9,0,0,+6].)
    const TWO_SEGMENT_V3 = {
      version: 3,
      sources: ["A.vue"],
      names: [],
      mappings: "AAAE,SAAM",
    };
    const dir = track(
      writeStore(manifestWithMap(), {
        "blobs/A.vue.tsx": "fooBarBaz = q;\n",
        "maps/A.vue.json": JSON.stringify(TWO_SEGMENT_V3),
      }),
    );
    const reader = new DiskCarrierStoreReader(dir);

    const remapped = remapCarrierSpan(
      reader,
      "d:/ws/src/A.vue.tsx",
      { start: 0, length: 9 },
      () => "fooBarBaz = q;\n",
      (sourcePath) => (sourcePath === "A.vue" ? "  fooBarBaz = q;\n" : undefined),
    );

    expect(remapped).not.toBeNull();
    expect(remapped!.fileName).toBe("A.vue");
    expect(remapped!.textSpan.start).toBe(2);
    expect(remapped!.textSpan.length).toBe(6);
  });

  it("a zero-length span maps to a zero-length source span (caret position)", () => {
    const dir = track(
      writeStore(manifestWithMap(), {
        "blobs/A.vue.tsx": "const x = 1;\n",
        "maps/A.vue.json": JSON.stringify(SINGLE_MAPPING_V3),
      }),
    );
    const reader = new DiskCarrierStoreReader(dir);
    const remapped = remapCarrierSpan(
      reader,
      "d:/ws/src/A.vue.tsx",
      { start: 0, length: 0 },
      () => "const x = 1;\n",
      () => "<source A.vue text>\n",
    );
    expect(remapped).not.toBeNull();
    expect(remapped!.textSpan.length).toBe(0);
  });

  it("returns null when the companion is not ready (fail closed)", () => {
    const dir = track(writeStore(manifestWithMap(), {}));
    const reader = new DiskCarrierStoreReader(dir);
    const remapped = remapCarrierSpan(
      reader,
      "d:/ws/src/NotReady.vue.tsx",
      { start: 0, length: 1 },
      () => "x",
      () => "y",
    );
    expect(remapped).toBeNull();
  });

  it("returns null when the carrier carries no map", () => {
    const m = manifestWithMap();
    delete m.projects["d:/ws/tsconfig.json"].ready_files["d:/ws/src/A.vue.tsx"].map_rel;
    const dir = track(writeStore(m, { "blobs/A.vue.tsx": "x" }));
    const reader = new DiskCarrierStoreReader(dir);
    const remapped = remapCarrierSpan(
      reader,
      "d:/ws/src/A.vue.tsx",
      { start: 0, length: 1 },
      () => "x",
      () => "y",
    );
    expect(remapped).toBeNull();
  });

  it("returns null when the source text cannot be read (fail closed)", () => {
    const dir = track(
      writeStore(manifestWithMap(), {
        "blobs/A.vue.tsx": "const x = 1;\n",
        "maps/A.vue.json": JSON.stringify(SINGLE_MAPPING_V3),
      }),
    );
    const reader = new DiskCarrierStoreReader(dir);
    const remapped = remapCarrierSpan(
      reader,
      "d:/ws/src/A.vue.tsx",
      { start: 0, length: 1 },
      () => "const x = 1;\n",
      () => undefined, // source unreadable
    );
    expect(remapped).toBeNull();
  });
});
