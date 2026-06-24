/**
 * The single producer-side redactor.
 *
 * Discriminating (all planted private strings built from runtime fragments so this
 * SOURCE stays clean): a planted real root → opaque id; a project-relative path →
 * opaque file id (neither root nor relative basename survives); a source map's
 * `sources` rewritten to `analysis://...` with `sourcesContent` omitted; a planted
 * TS diagnostic with an identifier SHAPE-redacted (no identifier survives). Clean
 * text (generic placeholders, `/tmp`, repo-relative) passes through unchanged.
 */

import { describe, expect, it } from "vitest";

import { Redactor, redactSourceMap, serializeRedactedJsonl } from "../src/redaction.js";

/** A planted private root, built from fragments. */
function plantedRoot(): string {
  return `/${"d:"}/${"dev"}/secret-corp/widgets`;
}

function redactor(): Redactor {
  return new Redactor([["p0001", plantedRoot()]]);
}

describe("Redactor", () => {
  it("redacts a root-prefixed path to an opaque token (no root, no basename)", () => {
    const r = redactor();
    const root = plantedRoot();
    const basename = "Button";
    const out = r.redactValue(`${root}/src/components/${basename}.vue`);
    expect(out.startsWith("analysis://p0001/file-")).toBe(true);
    expect(out.endsWith(".vue")).toBe(true);
    expect(out).not.toContain(root);
    expect(out).not.toContain(basename);
    expect(out).not.toContain("components");
  });

  it("gives a project-relative path a stable opaque file id", () => {
    const r = redactor();
    const root = plantedRoot();
    const a1 = r.sourceMapSource(`${root}/src/App.vue`);
    const a2 = r.sourceMapSource(`${root}/src/App.vue`);
    expect(a1).toBe("analysis://p0001/file-0001.vue");
    expect(a2).toBe(a1); // stable numbering
    expect(r.sourceMapSource(`${root}/src/Other.vue`)).toBe("analysis://p0001/file-0002.vue");
  });

  it("fails closed (null) for a path under no known root", () => {
    const r = redactor();
    expect(r.sourceMapSource("/path/to/generic/file.vue")).toBeNull();
  });

  it("redacts a back-slashed Windows root", () => {
    const r = redactor();
    const win = plantedRoot().replace(/\//g, "\\");
    const out = r.redactValue(`${win}\\src\\Widget.vue`);
    expect(out.startsWith("analysis://p0001/file-")).toBe(true);
    expect(out.toLowerCase()).not.toContain("widget");
  });

  it("leaves text without a known root unchanged", () => {
    const r = redactor();
    for (const neutral of [
      "/path/to/foo.vue",
      "/tmp/scratch.txt",
      "crates/verter_compiler/src/lib.rs",
      "analysis://p0001/file-0001.vue",
    ]) {
      expect(r.redactValue(neutral)).toBe(neutral);
    }
  });

  it("rewrites a source map's sources to opaque ids and omits sourcesContent", () => {
    const r = redactor();
    const root = plantedRoot();
    const map = {
      version: 3,
      sources: [`${root}/src/App.vue`],
      sourcesContent: ["<template>SECRET BODY</template>"],
      mappings: "AAAA",
    };
    const redacted = redactSourceMap(map, r);
    expect(redacted.sources).toEqual(["analysis://p0001/file-0001.vue"]);
    expect("sourcesContent" in redacted).toBe(false);
    // The serialized map carries neither the root nor the source body.
    const serialized = JSON.stringify(redacted);
    expect(serialized).not.toContain(root);
    expect(serialized).not.toContain("SECRET BODY");
  });

  it("fails closed when a source map has an unknown-root source", () => {
    const r = redactor();
    const map = { version: 3, sources: ["/path/to/unknown/App.vue"], mappings: "" };
    expect(() => redactSourceMap(map, r)).toThrow(/not under a known analysis root/);
  });

  it("shape-redacts a TS diagnostic so no identifier survives (TS code kept)", () => {
    const r = redactor();
    // A typical TS diagnostic carrying an identifier + a quoted name.
    const secretIdent = `${"Cur"}${"rency"}${"Codes"}`;
    const message = `error TS2304: Cannot find name '${secretIdent}'.`;
    const out = r.redactDiagnostic(message);
    // The identifier and the quoted name are gone; the shape + TS code remain.
    expect(out).not.toContain(secretIdent);
    expect(out).toContain("TS2304");
    expect(out).toContain("'<id>'"); // the quoted span collapsed to a placeholder
  });

  it("shape-redacts an import path inside a diagnostic", () => {
    const r = redactor();
    const root = plantedRoot();
    // A diagnostic embedding a real import path under a known root.
    const message = `Cannot find module '${root}/src/lib/secretModule.ts'.`;
    const out = r.redactDiagnostic(message);
    expect(out).not.toContain(root);
    expect(out).not.toContain("secretModule");
  });

  it("serializeRedactedJsonl emits one already-redacted record per line", () => {
    const records = [
      { id: "p0001", n: 1 },
      { id: "p0001", n: 2 },
    ];
    const out = serializeRedactedJsonl(records);
    expect(out).toBe('{"id":"p0001","n":1}\n{"id":"p0001","n":2}\n');
    expect(serializeRedactedJsonl([])).toBe("");
  });
});
