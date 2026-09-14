/**
 * Focused public Svelte IDE-projection TSX validity case.
 *
 * Regression (ECRS1): a component element that starts AT the first template
 * byte and owns an immediate `{#snippet}` child shared its IIFE anchor with
 * the render-header anchor, and the projected TSX did not parse (the header
 * landed inside the IIFE's `return (`). Minimized from the
 * `pikax/svelte-benchmarks` real-world corpus —
 * `hperrin/svelte-material-ui` @ 8d204fe859940871afa832dade80789bab49d752,
 * `packages/site/src/routes/demo/chips/_Input.svelte` — so the regression
 * runs in Verter with no benchmark checkout.
 *
 * The check mirrors the external corpus gate: the emitted IDE code must pass
 * the TypeScript TSX parser with zero parse diagnostics, and must retain the
 * authored bindings/expressions (snippet param `x`, sibling expression).
 */

import { describe, expect, it } from "vitest";
import ts from "typescript";

const native = require("../index.js") as typeof import("../index");

function parseTsxDiagnostics(filename: string, code: string) {
  const source = ts.createSourceFile(
    filename,
    code,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  return source.parseDiagnostics.map((diag) =>
    ts.flattenDiagnosticMessageText(diag.messageText, " "),
  );
}

describe("svelte ide projection tsx validity", () => {
  it("projects a first-template-byte component snippet scope as parseable TSX", () => {
    const host = new native.VerterHost({ analysisLevel: "full" });
    // No byte between the script close and the owning element: the element's
    // open tag starts exactly at the first template byte.
    const source =
      '<script lang="ts">import C from "./C.svelte";\nlet q = $state(1);</script>' +
      "<C>{#snippet s(x)}<p>{x}</p>{/snippet}</C>" +
      "<p>{q}</p>";
    host.upsert({ inputId: "Min.svelte", source, fileKind: "svelte" });

    expect(host.ensureIdeCompiled("Min.svelte")).toBe(true);

    const ide = host.getIde("Min.svelte");
    expect(ide).not.toBeNull();
    const code = ide!.code;
    expect(code).toContain("@verter/svelte-jsx");
    // The render header opens the render fragment BEFORE the element-snippet
    // IIFE — the inverted order was the invalid projection.
    const header = code.indexOf(";function __verter_render()");
    const iife = code.indexOf("{(() => {");
    expect(header).toBeGreaterThanOrEqual(0);
    expect(iife).toBeGreaterThan(header);
    // Authored bindings/expressions survive the projection.
    expect(code).toContain("{x}");
    expect(code).toContain("{q}");
    expect(code).not.toContain("{#snippet");

    expect(parseTsxDiagnostics("Min.svelte.tsx", code)).toEqual([]);
  });

  it("still rejects malformed TSX through the same parser check", () => {
    // The parser check itself must stay discriminating: a broken projection
    // shape (an unclosed element) is reported, not swallowed.
    const diags = parseTsxDiagnostics("Broken.tsx", "const v = (<div>unclosed;");
    expect(diags.length).toBeGreaterThan(0);
  });
});
