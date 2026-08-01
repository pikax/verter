import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, it, expect } from "vitest";
import ts from "typescript";
import type { OwnedSource } from "@verter/language-shared";
import { DiskCarrierStoreReader } from "./carrierStore";
import {
  carrierPathCompletionEntries,
  moduleSpecifierLiteralAt,
  type CarrierPathCompletionReader,
} from "./pathCompletion";

// ── moduleSpecifierLiteralAt ────────────────────────────────────────────────

function sourceFileOf(text: string): ts.SourceFile {
  return ts.createSourceFile("/ws/src/a.ts", text, ts.ScriptTarget.ESNext, true);
}

/** The literal found with the caret placed right after `./` inside `marker`. */
function literalAtFragment(text: string, marker: string): ts.StringLiteralLike | undefined {
  const index = text.indexOf(marker);
  if (index < 0) throw new Error(`marker ${marker} not in ${text}`);
  return moduleSpecifierLiteralAt(ts, sourceFileOf(text), index + marker.length);
}

describe("moduleSpecifierLiteralAt", () => {
  it("finds the specifier literal for every standard import shape", () => {
    expect(literalAtFragment('import X from "./";', '"./')?.text).toBe("./");
    expect(literalAtFragment('export { X } from "./";', '"./')?.text).toBe("./");
    expect(literalAtFragment('export * from "./";', '"./')?.text).toBe("./");
    expect(literalAtFragment('const p = import("./");', '"./')?.text).toBe("./");
    expect(literalAtFragment('const r = require("./");', '"./')?.text).toBe("./");
    expect(literalAtFragment('import fs = require("./");', '"./')?.text).toBe("./");
    expect(literalAtFragment('type T = typeof import("./");', '"./')?.text).toBe("./");
  });

  it("refuses non-specifier strings and non-string positions", () => {
    // A plain expression string is NOT a module specifier.
    expect(literalAtFragment('const s = "./";', '"./')).toBeUndefined();
    // An ordinary call argument is NOT a module specifier.
    expect(literalAtFragment('load("./");', '"./')).toBeUndefined();
    // The caret ON the opening quote (not inside the literal) refuses.
    const text = 'import X from "./";';
    expect(moduleSpecifierLiteralAt(ts, sourceFileOf(text), text.indexOf('"./'))).toBeUndefined();
    // An identifier position refuses.
    expect(moduleSpecifierLiteralAt(ts, sourceFileOf(text), text.indexOf("X"))).toBeUndefined();
  });
});

// ── carrierPathCompletionEntries ────────────────────────────────────────────

/**
 * A manifest-shaped reader over a fixed owned-source table. `canonicalPath`
 * lower-cases (a case-insensitive host), matching the disk reader's policy
 * surface. By default every `CarrierApi` row's import surface is READY (the
 * normal published state); `readyApiProviders` / `lastGood` override that to
 * model the warm-up window.
 */
function readerOf(
  owned: OwnedSource[],
  options?: { readyApiProviders?: string[]; lastGood?: string[] },
): CarrierPathCompletionReader {
  const canonicalPath = (path: string) => path.replace(/\\/g, "/").toLowerCase();
  const readyApiProviders = new Set(
    (
      options?.readyApiProviders ??
      owned.filter((entry) => entry.role === "CarrierApi").map((entry) => entry.provider_uri)
    ).map(canonicalPath),
  );
  const lastGood = new Set((options?.lastGood ?? []).map(canonicalPath));
  return {
    importCompletionSnapshot: () => ({ ownedSources: owned, readyApiProviders }),
    lastGoodBlobFor: (path: string) =>
      lastGood.has(canonicalPath(path)) ? "export default class LastGood {}" : undefined,
    canonicalPath,
  };
}

const apiRow = (source: string, provider: string): OwnedSource => ({
  source_uri: source,
  provider_uri: provider,
  role: "CarrierApi",
  script_kind: "TS",
});

const ideRow = (source: string, provider: string): OwnedSource => ({
  source_uri: source,
  provider_uri: provider,
  role: "CarrierIde",
  script_kind: "TSX",
});

function entriesFor(
  reader: CarrierPathCompletionReader,
  literalText: string,
  options?: { containingFile?: string; existingNames?: string[]; rawText?: string },
): ts.CompletionEntry[] {
  return carrierPathCompletionEntries({
    containingFile: options?.containingFile ?? "/ws/src/consumer.ts",
    literalText,
    // The RAW characters between the quotes; equals the cooked text unless the
    // authored specifier contains escape sequences.
    literalRawText: options?.rawText ?? literalText,
    literalStart: 17,
    reader,
    existingNames: new Set(options?.existingNames ?? []),
  });
}

describe("carrierPathCompletionEntries", () => {
  it("offers owned resolvable carriers in the fragment directory, both frameworks", () => {
    const reader = readerOf([
      apiRow("/ws/src/A.vue", "/ws/src/A.vue.verter.ts"),
      apiRow("/ws/src/W.svelte", "/ws/src/W.svelte.verter.ts"),
      apiRow("/ws/src/components/Nested.vue", "/ws/src/components/Nested.vue.verter.ts"),
    ]);
    const names = entriesFor(reader, "./").map((entry) => entry.name);
    expect(names).toEqual(["A.vue", "W.svelte"]);
    expect(names).not.toContain("Nested.vue");

    const nested = entriesFor(reader, "./components/").map((entry) => entry.name);
    expect(nested).toEqual(["Nested.vue"]);
  });

  it("resolves ../ fragments against the containing directory", () => {
    const reader = readerOf([apiRow("/ws/src/A.vue", "/ws/src/A.vue.verter.ts")]);
    const names = entriesFor(reader, "../src/", {
      containingFile: "/ws/nested/consumer.ts",
    }).map((entry) => entry.name);
    expect(names).toEqual(["A.vue"]);
  });

  it("fails closed: IDE-role-only rows, provider mismatches, and rune modules are never offered", () => {
    const reader = readerOf([
      // Import surface NOT owned — an ordinary import abstains, so no offer.
      ideRow("/ws/src/IdeOnly.vue", "/ws/src/IdeOnly.vue.tsx"),
      // A CarrierApi row whose provider is NOT the descriptor-derived import
      // surface — the resolve policy refuses it, so the offer must too.
      apiRow("/ws/src/Hijack.vue", "/ws/src/Other.vue.verter.ts"),
      // A rune module is a self-file module, never a component carrier.
      apiRow("/ws/src/store.svelte.ts", "/ws/src/store.svelte.ts"),
      // The one legitimate row.
      apiRow("/ws/src/A.vue", "/ws/src/A.vue.verter.ts"),
    ]);
    const names = entriesFor(reader, "./").map((entry) => entry.name);
    expect(names).toEqual(["A.vue"]);
  });

  it("never offers for a non-relative fragment and never duplicates existing names", () => {
    const reader = readerOf([apiRow("/ws/src/A.vue", "/ws/src/A.vue.verter.ts")]);
    expect(entriesFor(reader, "lodash")).toEqual([]);
    expect(entriesFor(reader, "")).toEqual([]);
    expect(entriesFor(reader, "./", { existingNames: ["A.vue"] })).toEqual([]);
  });

  it("shapes entries like TypeScript path completions", () => {
    const reader = readerOf([apiRow("/ws/src/W.svelte", "/ws/src/W.svelte.verter.ts")]);
    const [entry] = entriesFor(reader, "./");
    expect(entry.kind).toBe(ts.ScriptElementKind.scriptElement);
    expect(entry.kindModifiers).toBe(".svelte");
    expect(entry.sortText).toBe("11");
    // Identifier-text basename fragment → no replacement span (client-side
    // word replacement covers it), mirroring getDirectoryFragmentTextSpan.
    expect(entry.replacementSpan).toBeUndefined();
    const [withSpan] = entriesFor(reader, "./W.sv");
    // literalStart 17 → fragment basename `W.sv` starts after `./` at 17+1+2.
    expect(withSpan.replacementSpan).toEqual({ start: 20, length: 4 });
  });

  // ── replacement spans address RAW source characters (cooked ≠ raw) ────────
  //
  // TypeScript's `node.text` is the COOKED value: escaped separators collapse
  // (`.\\W.sv` in source → `.\W.sv` cooked), so a span computed from cooked
  // offsets addresses the wrong raw characters and accepting an entry CORRUPTS
  // the user's import (`".\W.sveltev"`). Spans must come from the raw text.
  describe("raw-vs-cooked replacement spans", () => {
    const reader = () => readerOf([apiRow("/ws/src/W.svelte", "/ws/src/W.svelte.verter.ts")]);

    it("windows-style escaped-backslash fragment spans the RAW basename", () => {
      // Source: import W from ".\\W.sv"  — raw contents `.\\W.sv` (7 chars),
      // cooked `.\W.sv` (6 chars). Raw basename `W.sv` starts at raw offset 3.
      const [entry] = entriesFor(reader(), ".\\W.sv", { rawText: ".\\\\W.sv" });
      expect(entry).toBeDefined();
      // literalStart 17 → opening quote at 17, raw basename at 17+1+3.
      expect(entry.replacementSpan).toEqual({ start: 21, length: 4 });
    });

    it("escaped-forward-slash fragment spans the RAW basename", () => {
      // Source: import W from ".\/W.sv" — raw `.\/W.sv`, cooked `./W.sv`.
      const [entry] = entriesFor(reader(), "./W.sv", { rawText: ".\\/W.sv" });
      expect(entry).toBeDefined();
      expect(entry.replacementSpan).toEqual({ start: 21, length: 4 });
    });

    it("fails closed on escape forms it cannot address (unicode escape in basename)", () => {
      // Source: import W from "./W.sv" — cooked `./W.sv`, raw basename
      // `W.sv`. No trustworthy raw span exists without a full cooked↔raw
      // map, and an entry with a cooked-offset span (or none at all) corrupts
      // on accept — so no carrier entry is offered at this position.
      expect(entriesFor(reader(), "./W.sv", { rawText: "./\\u0057.sv" })).toEqual([]);
    });

    it("windows-style fragment ending at the separator offers with no span", () => {
      // Source: import W from ".\\" — raw `.\\`, cooked `.\`; empty basename →
      // plain insertion at the caret, exactly like the `./` posix case.
      const [entry] = entriesFor(reader(), ".\\", { rawText: ".\\\\" });
      expect(entry).toBeDefined();
      expect(entry.name).toBe("W.svelte");
      expect(entry.replacementSpan).toBeUndefined();
    });
  });

  // ── keystroke-path I/O bound: ONE manifest read per request ───────────────
  it("reads the manifest exactly once regardless of candidate count (500 carriers)", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "verter-pathcomp-"));
    try {
      const owned: OwnedSource[] = [];
      const ready: Record<string, unknown> = {};
      for (let index = 0; index < 500; index += 1) {
        const source = `/ws/src/C${index}.vue`;
        const provider = `${source}.verter.ts`;
        owned.push({
          source_uri: source,
          provider_uri: provider,
          role: "CarrierApi",
          script_kind: "TS",
        });
        ready[provider] = {
          content_hash: `h${index}`,
          version: 1,
          script_kind: "TS",
          role: "CarrierApi",
          map_hash: "0",
          blob_rel: `blobs/C${index}`,
        };
      }
      fs.writeFileSync(
        path.join(dir, "manifest.json"),
        JSON.stringify({
          epoch: 1,
          host_version: "test",
          projects: { "/ws/tsconfig.json": { owned_sources: owned, ready_files: ready } },
        }),
      );
      const diskReader = new DiskCarrierStoreReader(dir, "/ws/tsconfig.json");
      let manifestReads = 0;
      const originalReadManifest = DiskCarrierStoreReader.prototype.readManifest;
      (diskReader as { readManifest(): unknown }).readManifest = function (
        this: DiskCarrierStoreReader,
      ) {
        manifestReads += 1;
        return originalReadManifest.call(this);
      };

      const entries = carrierPathCompletionEntries({
        containingFile: "/ws/src/consumer.ts",
        literalText: "./",
        literalRawText: "./",
        literalStart: 17,
        reader: diskReader,
        existingNames: new Set(),
      });

      // Every candidate was genuinely processed (guards against a "fix" that
      // trivially returns early) …
      expect(entries).toHaveLength(500);
      // … and completion fires on keystrokes, so the request does BOUNDED
      // manifest I/O: exactly ONE read (one `statSync`), never one per
      // candidate (1 + 500 before the fix).
      expect(manifestReads).toBe(1);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  // ── warm-up: ownership precedes publication ───────────────────────────────
  //
  // Owned rows exist BEFORE content publishes (`owned_sources` lands, then
  // `ready_files`), and `resolveCarrierImportTarget` deliberately does not
  // decide readiness. The offer gate therefore additionally requires what the
  // NON-BLOCKING resolution arms require: a ready `CarrierApi` surface in the
  // same manifest snapshot, or retained last-good content.
  describe("warm-up readiness gate", () => {
    it("withholds an owned carrier whose import surface has not published yet", () => {
      const reader = readerOf(
        [
          apiRow("/ws/src/Warm.vue", "/ws/src/Warm.vue.verter.ts"),
          apiRow("/ws/src/Ready.vue", "/ws/src/Ready.vue.verter.ts"),
        ],
        { readyApiProviders: ["/ws/src/Ready.vue.verter.ts"] },
      );
      const names = entriesFor(reader, "./").map((entry) => entry.name);
      expect(names).toEqual(["Ready.vue"]);
      expect(names).not.toContain("Warm.vue");
    });

    it("a retained last-good surface keeps the carrier offered across a not-ready window", () => {
      const reader = readerOf([apiRow("/ws/src/A.vue", "/ws/src/A.vue.verter.ts")], {
        readyApiProviders: [],
        lastGood: ["/ws/src/A.vue.verter.ts"],
      });
      expect(entriesFor(reader, "./").map((entry) => entry.name)).toEqual(["A.vue"]);
    });

    it("no ready surface and no last-good means nothing is offered", () => {
      const reader = readerOf([apiRow("/ws/src/A.vue", "/ws/src/A.vue.verter.ts")], {
        readyApiProviders: [],
      });
      expect(entriesFor(reader, "./")).toEqual([]);
    });
  });
});
