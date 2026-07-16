import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, utimesSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { CarrierStoreReader, Manifest } from "@verter/language-shared";
import {
  DiskCarrierStoreReader,
  resolveCarrierStoreDir,
  resolveResponseRemap,
} from "./carrierStore";

/** Write a manifest + the named blob/map files into a fresh store dir. */
function makeStore(manifest: Manifest, blobs: Record<string, string> = {}): string {
  const dir = mkdtempSync(join(tmpdir(), "verter-carrier-store-"));
  mkdirSync(join(dir, "blobs"), { recursive: true });
  mkdirSync(join(dir, "maps"), { recursive: true });
  for (const [rel, content] of Object.entries(blobs)) {
    const abs = join(dir, rel);
    mkdirSync(join(abs, ".."), { recursive: true });
    writeFileSync(abs, content, "utf8");
  }
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest), "utf8");
  return dir;
}

const baseManifest = (): Manifest => ({
  epoch: 7,
  host_version: "test-host",
  projects: {
    "d:/ws/tsconfig.json": {
      owned_sources: [
        {
          source_uri: "d:/ws/src/A.vue",
          provider_uri: "d:/ws/src/A.vue.tsx",
          role: "CarrierIde",
          script_kind: "TSX",
        },
        {
          source_uri: "d:/ws/src/B.vue",
          provider_uri: "d:/ws/src/B.vue.tsx",
          role: "CarrierIde",
          script_kind: "TSX",
        },
      ],
      ready_files: {
        "d:/ws/src/A.vue.tsx": {
          content_hash: "aaaa",
          version: 3,
          script_kind: "TSX",
          role: "CarrierIde",
          map_hash: "bbbb",
          blob_rel: "blobs/blake3-aaaa.tsx",
          map_rel: "maps/blake3-bbbb.json",
        },
      },
    },
  },
});

/**
 * A manifest with TWO configured projects, each owning + publishing a DIFFERENT
 * carrier (a Vue carrier in project A, a Svelte carrier in project B). Used to
 * prove the project-scoped reader never crosses tsconfig boundaries.
 */
const twoProjectManifest = (): Manifest => ({
  epoch: 4,
  host_version: "test-host",
  projects: {
    "d:/ws/a/tsconfig.json": {
      owned_sources: [
        {
          source_uri: "d:/ws/a/src/A.vue",
          provider_uri: "d:/ws/a/src/A.vue.tsx",
          role: "CarrierIde",
          script_kind: "TSX",
        },
      ],
      ready_files: {
        "d:/ws/a/src/A.vue.tsx": {
          content_hash: "a1",
          version: 1,
          script_kind: "TSX",
          role: "CarrierIde",
          map_hash: "0",
          blob_rel: "blobs/A.vue.tsx",
        },
      },
    },
    "d:/ws/b/tsconfig.json": {
      owned_sources: [
        {
          source_uri: "d:/ws/b/src/B.svelte",
          provider_uri: "d:/ws/b/src/B.svelte.tsx",
          role: "CarrierIde",
          script_kind: "TSX",
        },
      ],
      ready_files: {
        "d:/ws/b/src/B.svelte.tsx": {
          content_hash: "b1",
          version: 1,
          script_kind: "TSX",
          role: "CarrierIde",
          map_hash: "0",
          blob_rel: "blobs/B.svelte.tsx",
        },
      },
    },
  },
});

let dirs: string[] = [];
function track(dir: string): string {
  dirs.push(dir);
  return dir;
}

beforeEach(() => {
  dirs = [];
});
afterEach(() => {
  for (const d of dirs) {
    rmSync(d, { recursive: true, force: true });
  }
  delete process.env.VERTER_CARRIER_STORE_DIR;
  delete process.env.VERTER_PLUGIN_RESPONSE_REMAP;
});

describe("resolveCarrierStoreDir", () => {
  it("prefers the plugin config carrierStoreDir", () => {
    expect(resolveCarrierStoreDir({ carrierStoreDir: "/from/config" })).toBe("/from/config");
  });
  it("falls back to the VERTER_CARRIER_STORE_DIR env", () => {
    process.env.VERTER_CARRIER_STORE_DIR = "/from/env";
    expect(resolveCarrierStoreDir(undefined)).toBe("/from/env");
    expect(resolveCarrierStoreDir({})).toBe("/from/env");
  });
  it("config wins over env", () => {
    process.env.VERTER_CARRIER_STORE_DIR = "/from/env";
    expect(resolveCarrierStoreDir({ carrierStoreDir: "/from/config" })).toBe("/from/config");
  });
  it("returns undefined when neither is set", () => {
    expect(resolveCarrierStoreDir(undefined)).toBeUndefined();
    expect(resolveCarrierStoreDir({})).toBeUndefined();
  });
  it("ignores an empty-string config/env", () => {
    expect(resolveCarrierStoreDir({ carrierStoreDir: "" })).toBeUndefined();
  });
});

describe("resolveResponseRemap", () => {
  it("defaults to ENABLED (the VS Code direct surface) when unset", () => {
    expect(resolveResponseRemap(undefined)).toBe(true);
    expect(resolveResponseRemap({})).toBe(true);
  });
  it("a boolean plugin config wins outright", () => {
    expect(resolveResponseRemap({ responseRemap: false })).toBe(false);
    expect(resolveResponseRemap({ responseRemap: true })).toBe(true);
  });
  it("the VERTER_PLUGIN_RESPONSE_REMAP env disables on `0` / `false`", () => {
    process.env.VERTER_PLUGIN_RESPONSE_REMAP = "0";
    expect(resolveResponseRemap(undefined)).toBe(false);
    process.env.VERTER_PLUGIN_RESPONSE_REMAP = "false";
    expect(resolveResponseRemap({})).toBe(false);
    process.env.VERTER_PLUGIN_RESPONSE_REMAP = "FALSE";
    expect(resolveResponseRemap(undefined)).toBe(false);
  });
  it("the env enables on `1` / `true`", () => {
    process.env.VERTER_PLUGIN_RESPONSE_REMAP = "1";
    expect(resolveResponseRemap(undefined)).toBe(true);
    process.env.VERTER_PLUGIN_RESPONSE_REMAP = "true";
    expect(resolveResponseRemap(undefined)).toBe(true);
  });
  it("config wins over env", () => {
    process.env.VERTER_PLUGIN_RESPONSE_REMAP = "1";
    expect(resolveResponseRemap({ responseRemap: false })).toBe(false);
  });
  it("an unrecognized env value falls back to the default (enabled)", () => {
    process.env.VERTER_PLUGIN_RESPONSE_REMAP = "maybe";
    expect(resolveResponseRemap(undefined)).toBe(true);
  });
});

describe("DiskCarrierStoreReader.isAvailable", () => {
  it("is false with no store dir", () => {
    expect(new DiskCarrierStoreReader(undefined).isAvailable()).toBe(false);
  });
  it("is true with a store dir", () => {
    expect(new DiskCarrierStoreReader("/anything").isAvailable()).toBe(true);
  });
});

describe("DiskCarrierStoreReader.readManifest", () => {
  it("reads and parses the manifest", () => {
    const dir = track(makeStore(baseManifest()));
    const reader = new DiskCarrierStoreReader(dir);
    const m = reader.readManifest();
    expect(m?.epoch).toBe(7);
    expect(m?.host_version).toBe("test-host");
    expect(Object.keys(m!.projects)).toEqual(["d:/ws/tsconfig.json"]);
  });

  it("returns undefined for a missing manifest (store not warmed)", () => {
    const dir = track(mkdtempSync(join(tmpdir(), "verter-empty-store-")));
    const reader = new DiskCarrierStoreReader(dir);
    expect(reader.readManifest()).toBeUndefined();
    expect(reader.currentEpoch()).toBeUndefined();
  });

  it("returns undefined when no store dir is configured", () => {
    expect(new DiskCarrierStoreReader(undefined).readManifest()).toBeUndefined();
  });

  it("tolerates a torn (unparseable) manifest without throwing", () => {
    const dir = track(makeStore(baseManifest()));
    // Overwrite with a half-written JSON.
    writeFileSync(join(dir, "manifest.json"), '{ "epoch": 7, "projects":', "utf8");
    const reader = new DiskCarrierStoreReader(dir);
    expect(() => reader.readManifest()).not.toThrow();
    expect(reader.readManifest()).toBeUndefined();
  });

  it("caches by mtime/size and re-reads only when the file changes", () => {
    const dir = track(makeStore(baseManifest()));
    const reader = new DiskCarrierStoreReader(dir);
    expect(reader.readManifest()?.epoch).toBe(7);

    // Rewrite with a new epoch + a bumped mtime so the change is detected.
    const next = baseManifest();
    next.epoch = 9;
    writeFileSync(join(dir, "manifest.json"), JSON.stringify(next), "utf8");
    const future = Date.now() / 1000 + 100;
    utimesSync(join(dir, "manifest.json"), future, future);

    expect(reader.readManifest()?.epoch).toBe(9);
  });
});

describe("DiskCarrierStoreReader.ownedSources", () => {
  it("returns all owned sources across projects", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    const owned = reader.ownedSources();
    expect(owned.map((o) => o.source_uri).sort()).toEqual(["d:/ws/src/A.vue", "d:/ws/src/B.vue"]);
  });
  it("restricts to one project", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.ownedSources("d:/ws/tsconfig.json")).toHaveLength(2);
    expect(reader.ownedSources("d:/other/tsconfig.json")).toHaveLength(0);
  });
  it("is empty when the store is unavailable", () => {
    expect(new DiskCarrierStoreReader(undefined).ownedSources()).toHaveLength(0);
  });
});

describe("DiskCarrierStoreReader.readyFile", () => {
  it("finds a ready file by provider path", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    const rf = reader.readyFile("d:/ws/src/A.vue.tsx");
    expect(rf?.content_hash).toBe("aaaa");
    expect(rf?.version).toBe(3);
    expect(rf?.blob_rel).toBe("blobs/blake3-aaaa.tsx");
  });
  it("normalizes backslash paths", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.readyFile("d:\\ws\\src\\A.vue.tsx")?.version).toBe(3);
  });
  it("returns undefined for an owned-but-not-ready provider", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    // B is owned but not in ready_files.
    expect(reader.readyFile("d:/ws/src/B.vue.tsx")).toBeUndefined();
  });
  it("returns undefined for an unknown provider", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.readyFile("d:/ws/src/Unknown.vue.tsx")).toBeUndefined();
  });
});

describe("DiskCarrierStoreReader.readyFileForSource", () => {
  // `getExternalFiles` advertises the SOURCE path (`A.vue`) to tsserver, which
  // then queries the host for the SOURCE path's snapshot/kind/version. The reader
  // maps the source path to its IDE companion's (`A.vue.tsx`) ready entry — the
  // membership-identity reconciliation.
  it("maps a carrier SOURCE path to its IDE companion's ready entry", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    const rf = reader.readyFileForSource("d:/ws/src/A.vue");
    expect(rf?.content_hash).toBe("aaaa");
    expect(rf?.version).toBe(3);
    expect(rf?.blob_rel).toBe("blobs/blake3-aaaa.tsx");
  });
  it("returns undefined for a source whose companion is owned-but-not-ready", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    // B.vue is owned but its B.vue.tsx companion is not in ready_files.
    expect(reader.readyFileForSource("d:/ws/src/B.vue")).toBeUndefined();
  });
  it("returns undefined for a non-carrier path (a plain .ts source)", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    // A plain `.ts` is not a carrier source — it must not map to a companion.
    expect(reader.readyFileForSource("d:/ws/src/util.ts")).toBeUndefined();
  });
  it("does NOT treat a companion path as a source (no double-mapping)", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    // A `.vue.tsx` companion path is not a SOURCE path; readyFileForSource maps
    // only bare carrier sources, so it must not resolve the companion here.
    expect(reader.readyFileForSource("d:/ws/src/A.vue.tsx")).toBeUndefined();
  });
  it("companionForSource derives the IDE companion for a carrier source", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.companionForSource("d:/ws/src/A.vue")).toBe("d:/ws/src/A.vue.tsx");
    expect(reader.companionForSource("d:/ws/src/util.ts")).toBeUndefined();
  });
  it("uses the manifest IDE identity for a JavaScript Svelte carrier", () => {
    const manifest = baseManifest();
    manifest.projects["d:/ws/tsconfig.json"].owned_sources.push({
      source_uri: "d:/ws/src/JsWidget.svelte",
      provider_uri: "d:/ws/src/JsWidget.svelte.jsx",
      role: "CarrierIde",
      script_kind: "JSX",
    });
    manifest.projects["d:/ws/tsconfig.json"].ready_files["d:/ws/src/JsWidget.svelte.jsx"] = {
      content_hash: "js1",
      version: 5,
      script_kind: "JSX",
      role: "CarrierIde",
      map_hash: "0",
      blob_rel: "blobs/JsWidget.svelte.jsx",
    };
    const reader = new DiskCarrierStoreReader(track(makeStore(manifest)));

    expect(reader.companionForSource("d:/ws/src/JsWidget.svelte")).toBe(
      "d:/ws/src/JsWidget.svelte.jsx",
    );
    expect(reader.readyFileForSource("d:/ws/src/JsWidget.svelte")?.script_kind).toBe("JSX");
    expect(reader.readyFileForSource("d:/ws/src/JsWidget.svelte")?.version).toBe(5);
  });

  it("selects one public API carrier independently of a Svelte source dialect", () => {
    const manifest = baseManifest();
    const project = manifest.projects["d:/ws/tsconfig.json"];
    project.owned_sources.push(
      {
        source_uri: "d:/ws/src/JsWidget.svelte",
        provider_uri: "d:/ws/src/JsWidget.svelte.jsx",
        role: "CarrierIde",
        script_kind: "JSX",
      },
      {
        source_uri: "d:/ws/src/JsWidget.svelte",
        provider_uri: "d:/ws/src/JsWidget.svelte.verter.ts",
        role: "CarrierApi",
        script_kind: "TS",
      },
    );
    project.ready_files["d:/ws/src/JsWidget.svelte.verter.ts"] = {
      content_hash: "public-api",
      version: 1,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/JsWidget.svelte.verter.ts",
    };
    const reader = new DiskCarrierStoreReader(track(makeStore(manifest)));

    expect(reader.companionForSource("d:/ws/src/JsWidget.svelte")).toBe(
      "d:/ws/src/JsWidget.svelte.jsx",
    );
    expect(reader.apiCompanionForSource("d:/ws/src/JsWidget.svelte")).toBe(
      "d:/ws/src/JsWidget.svelte.verter.ts",
    );

    delete project.ready_files["d:/ws/src/JsWidget.svelte.verter.ts"];
    const unreadyReader = new DiskCarrierStoreReader(track(makeStore(manifest)));
    expect(unreadyReader.apiCompanionForSource("d:/ws/src/JsWidget.svelte")).toBeUndefined();
  });

  it("uses the manifest IDE identity for a JavaScript carrier", () => {
    const manifest = baseManifest();
    const project = manifest.projects["d:/ws/tsconfig.json"];
    const owned = project.owned_sources.find((entry) => entry.source_uri.endsWith("/A.vue"));
    expect(owned).toBeDefined();
    owned!.provider_uri = "d:/ws/src/A.vue.jsx";
    owned!.script_kind = "JSX";
    const ready = project.ready_files["d:/ws/src/A.vue.tsx"];
    delete project.ready_files["d:/ws/src/A.vue.tsx"];
    project.ready_files["d:/ws/src/A.vue.jsx"] = {
      ...ready,
      script_kind: "JSX",
      blob_rel: "blobs/blake3-aaaa.jsx",
    };
    const reader = new DiskCarrierStoreReader(track(makeStore(manifest)));

    expect(reader.companionForSource("d:/ws/src/A.vue")).toBe("d:/ws/src/A.vue.jsx");
    expect(reader.readyFileForSource("d:/ws/src/A.vue")?.script_kind).toBe("JSX");
  });
});

describe("DiskCarrierStoreReader.ownedSourceFor", () => {
  it("matches by provider path", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.ownedSourceFor("d:/ws/src/B.vue.tsx")?.source_uri).toBe("d:/ws/src/B.vue");
  });
  it("matches by source path", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.ownedSourceFor("d:/ws/src/B.vue")?.provider_uri).toBe("d:/ws/src/B.vue.tsx");
  });
  it("returns undefined for an unknown path", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.ownedSourceFor("d:/ws/src/Nope.vue")).toBeUndefined();
  });
});

describe("DiskCarrierStoreReader.readBlobSync", () => {
  it("reads a blob's content", () => {
    const dir = track(
      makeStore(baseManifest(), { "blobs/blake3-aaaa.tsx": "export const A = 1;" }),
    );
    const reader = new DiskCarrierStoreReader(dir);
    expect(reader.readBlobSync("blobs/blake3-aaaa.tsx")).toBe("export const A = 1;");
  });
  it("returns undefined for a missing blob", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.readBlobSync("blobs/blake3-missing.tsx")).toBeUndefined();
  });
  it("retains last-good when a provider path is given", () => {
    const dir = track(
      makeStore(baseManifest(), { "blobs/blake3-aaaa.tsx": "export const A = 1;" }),
    );
    const reader = new DiskCarrierStoreReader(dir);
    reader.readBlobSync("blobs/blake3-aaaa.tsx", "d:/ws/src/A.vue.tsx");
    expect(reader.lastGoodBlobFor("d:/ws/src/A.vue.tsx")).toBe("export const A = 1;");
  });
});

describe("DiskCarrierStoreReader.readMapSync", () => {
  it("reads and parses the map JSON", () => {
    const mapJson = '{"version":3,"sources":["A.vue"],"names":[],"mappings":"AAAA"}';
    const dir = track(makeStore(baseManifest(), { "maps/blake3-bbbb.json": mapJson }));
    const reader = new DiskCarrierStoreReader(dir);
    const map = reader.readMapSync("maps/blake3-bbbb.json") as { version: number };
    expect(map.version).toBe(3);
  });
  it("returns undefined for a missing map", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.readMapSync("maps/blake3-missing.json")).toBeUndefined();
  });
});

describe("DiskCarrierStoreReader.currentEpoch", () => {
  it("returns the manifest epoch", () => {
    const reader = new DiskCarrierStoreReader(track(makeStore(baseManifest())));
    expect(reader.currentEpoch()).toBe(7);
  });
});

describe("DiskCarrierStoreReader project scoping (no cross-tsconfig leak)", () => {
  it("a project-scoped reader only sees its OWN project's ready files", () => {
    const dir = track(makeStore(twoProjectManifest()));
    const readerA = new DiskCarrierStoreReader(dir, "d:/ws/a/tsconfig.json");
    const readerB = new DiskCarrierStoreReader(dir, "d:/ws/b/tsconfig.json");

    // Each reader resolves ITS OWN carrier…
    expect(readerA.readyFile("d:/ws/a/src/A.vue.tsx")?.content_hash).toBe("a1");
    expect(readerB.readyFile("d:/ws/b/src/B.svelte.tsx")?.content_hash).toBe("b1");
    // …and NEVER the sibling project's carrier.
    expect(readerA.readyFile("d:/ws/b/src/B.svelte.tsx")).toBeUndefined();
    expect(readerB.readyFile("d:/ws/a/src/A.vue.tsx")).toBeUndefined();
  });

  it("readyIdeSources is project-scoped — only the OWN project's source identities", () => {
    const dir = track(makeStore(twoProjectManifest()));
    const readerA = new DiskCarrierStoreReader(dir, "d:/ws/a/tsconfig.json");
    const readerB = new DiskCarrierStoreReader(dir, "d:/ws/b/tsconfig.json");

    expect(readerA.readyIdeSources()).toEqual(["d:/ws/a/src/A.vue"]);
    expect(readerB.readyIdeSources()).toEqual(["d:/ws/b/src/B.svelte"]);
  });

  it("ownedSourceFor is project-scoped — a sibling project's owned source is invisible", () => {
    const dir = track(makeStore(twoProjectManifest()));
    const readerA = new DiskCarrierStoreReader(dir, "d:/ws/a/tsconfig.json");
    expect(readerA.ownedSourceFor("d:/ws/a/src/A.vue")?.provider_uri).toBe("d:/ws/a/src/A.vue.tsx");
    // The Svelte carrier belongs to project B — reader A must not see it.
    expect(readerA.ownedSourceFor("d:/ws/b/src/B.svelte")).toBeUndefined();
    expect(readerA.ownedSourceFor("d:/ws/b/src/B.svelte.tsx")).toBeUndefined();
  });

  it("an UNSCOPED reader still spans every project (legitimate cross-project use)", () => {
    const dir = track(makeStore(twoProjectManifest()));
    const reader = new DiskCarrierStoreReader(dir);
    expect(reader.readyFile("d:/ws/a/src/A.vue.tsx")?.content_hash).toBe("a1");
    expect(reader.readyFile("d:/ws/b/src/B.svelte.tsx")?.content_hash).toBe("b1");
    expect(reader.readyIdeSources().sort()).toEqual(["d:/ws/a/src/A.vue", "d:/ws/b/src/B.svelte"]);
  });

  it("scopes by the normalized project key (backslash + drive-letter case fold)", () => {
    const dir = track(makeStore(twoProjectManifest()));
    // tsserver may hand `getProjectName()` back with backslashes or a different
    // drive-letter case than the Rust-written manifest key — both still resolve.
    const readerBackslash = new DiskCarrierStoreReader(dir, "d:\\ws\\a\\tsconfig.json");
    expect(readerBackslash.readyFile("d:/ws/a/src/A.vue.tsx")?.content_hash).toBe("a1");
    const readerUpperDrive = new DiskCarrierStoreReader(dir, "D:/ws/a/tsconfig.json");
    expect(readerUpperDrive.readyFile("d:/ws/a/src/A.vue.tsx")?.content_hash).toBe("a1");
    // The case-fold must NOT collapse distinct projects: project A's reader
    // still cannot see project B's carrier.
    expect(readerUpperDrive.readyFile("d:/ws/b/src/B.svelte.tsx")).toBeUndefined();
  });

  it("resolves source and provider identities using the host filesystem case policy", () => {
    const dir = track(makeStore(twoProjectManifest()));
    const insensitive = new DiskCarrierStoreReader(dir, "D:\\ws\\a\\tsconfig.json", false);

    expect(insensitive.companionForSource("D:\\WS\\A\\SRC\\A.VUE")).toBe("d:/ws/a/src/A.vue.tsx");
    expect(insensitive.readyFileForSource("D:\\WS\\A\\SRC\\A.VUE")?.content_hash).toBe("a1");
    expect(insensitive.readyFile("D:\\WS\\A\\SRC\\A.VUE.TSX")?.content_hash).toBe("a1");
    expect(insensitive.ownedSourceFor("D:\\WS\\A\\SRC\\A.VUE")?.provider_uri).toBe(
      "d:/ws/a/src/A.vue.tsx",
    );

    const sensitive = new DiskCarrierStoreReader(dir, "d:/ws/a/tsconfig.json", true);
    expect(sensitive.readyFileForSource("D:/WS/A/SRC/A.VUE")).toBeUndefined();
    expect(sensitive.readyFile("D:/WS/A/SRC/A.VUE.TSX")).toBeUndefined();
    expect(sensitive.ownedSourceFor("D:/WS/A/SRC/A.VUE")).toBeUndefined();
  });

  it("an unknown project key resolves to an EMPTY scope (fail closed)", () => {
    const dir = track(makeStore(twoProjectManifest()));
    const reader = new DiskCarrierStoreReader(dir, "d:/ws/unknown/tsconfig.json");
    expect(reader.readyIdeSources()).toEqual([]);
    expect(reader.readyFile("d:/ws/a/src/A.vue.tsx")).toBeUndefined();
  });
});
