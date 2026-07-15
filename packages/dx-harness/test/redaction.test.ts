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
      // Repo-relative paths whose segments merely CONTAIN users/home/a drive-looking
      // token must NOT be redacted (the `\b`/lookbehind word boundary is mirrored).
      "src/Users/widget.ts",
      "crates/verter_lsp/src/Users.rs",
      "packages/home/index.ts",
      "myhome/users/x.ts",
    ]) {
      expect(r.redactValue(neutral)).toBe(neutral);
    }
  });

  // B-b: multi-root ordering. The SHORTER root appears EARLIER in the string; the
  // old "first root with any match wins" rule sliced everything before the LONGER
  // root verbatim, leaking the shorter root. Both must redact regardless of order.
  function twoRootRedactor(): { r: Redactor; short: string; long: string } {
    const short = `/${"d:"}/${"dev"}/alpha`;
    const long = `/${"d:"}/${"dev"}/beta-corp/widgets`;
    return {
      r: new Redactor([
        ["p0001", short],
        ["p0002", long],
      ]),
      short,
      long,
    };
  }

  it("redacts all known roots regardless of order in the string", () => {
    const { r, short, long } = twoRootRedactor();
    const out = r.redactValue(`a ${short}/src/A.vue then b ${long}/src/B.vue end`);
    expect(out).not.toContain(short);
    expect(out).not.toContain(long);
    expect(out).toContain("analysis://p0001/file-");
    expect(out).toContain("analysis://p0002/file-");
    const out2 = r.redactValue(`x ${long}/src/B.vue y ${short}/src/A.vue z`);
    expect(out2).not.toContain(short);
    expect(out2).not.toContain(long);
  });

  it("a nested root still wins over its ancestor at the same position", () => {
    const ancestor = `/${"d:"}/${"dev"}/mono`;
    const nested = `${ancestor}/packages/ui`;
    const r = new Redactor([
      ["p0001", ancestor],
      ["p0002", nested],
    ]);
    const out = r.redactValue(`${nested}/src/Comp.vue`);
    expect(out.startsWith("analysis://p0002/file-")).toBe(true);
    expect(out).not.toContain(ancestor);
  });

  // C-b: fail-closed for UNKNOWN-root absolute-path shapes. A private path under a
  // root the redactor was NOT configured with must never ride out verbatim.
  it("redactValue fails closed on unknown-root absolute-path shapes", () => {
    const r = redactor();
    const secret = "Sekret";
    const cases = [
      `/Users/alice/proj/src/${secret}.vue`,
      `/home/bob/app/src/${secret}.ts`,
      `c:/dev/other-corp/${secret}.tsx`,
      `c:/Users/carol/work/${secret}.vue`,
      `file:///Users/dave/x/${secret}.ts`,
    ];
    for (const input of cases) {
      const out = r.redactValue(input);
      expect(out, `input ${input}`).toContain("analysis://unknown");
      expect(out.toLowerCase()).not.toContain(secret.toLowerCase());
    }
  });

  it("fail-closed consumes a basename after a comma/bracket with no tail leak", () => {
    const r = redactor();
    const secret = "Sekret";
    // unknown root, comma in a segment
    const out1 = r.redactValue(`/Users/al/My,Docs/${secret}.ts`);
    expect(out1.toLowerCase()).not.toContain(secret.toLowerCase());
    expect(out1).not.toContain("Docs");
    // unknown root, bracket in a segment
    const out2 = r.redactValue(`(/home/bo/a]b/${secret}.vue)`);
    expect(out2.toLowerCase()).not.toContain(secret.toLowerCase());
    // KNOWN root, comma in a segment → folds into the opaque id, no tail
    const root = plantedRoot();
    const out3 = r.redactValue(`${root}/My,Docs/${secret}.ts`);
    expect(out3.toLowerCase()).not.toContain(secret.toLowerCase());
    expect(out3).not.toContain("Docs");
    expect(out3).toContain("analysis://p0001/file-");
  });

  it("fail-closed redaction embedded in a message keeps neutral text", () => {
    const r = redactor();
    const out = r.redactValue("error TS2307 at /Users/eve/secret/main.ts: cannot find module");
    expect(out).not.toContain("/Users/eve");
    expect(out).not.toContain("secret");
    expect(out).toContain("analysis://unknown");
    expect(out).toContain("error TS2307 at");
    expect(out).toContain("cannot find module");
  });

  it("fails closed on a bare file:/// home URI with no trailing slash", () => {
    // Mirrors the leak guard: `file:///users/`|`file:///home/` matched on the marker
    // alone, so `file:///Users/alice` (no further slash) must redact, not survive.
    const r = redactor();
    for (const input of ["file:///Users/alice", "file:///home/bob"]) {
      expect(r.redactValue(input)).toBe("analysis://unknown");
    }
  });

  it("displayPath fails closed for an unknown-root private shape", () => {
    const r = redactor();
    const shown = r.displayPath("/Users/frank/app/src/Secret.vue");
    expect(shown).toBe("analysis://unknown");
    expect(shown).not.toContain("frank");
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
