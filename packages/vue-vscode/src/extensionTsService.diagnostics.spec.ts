// Headless regression guard for the EXTENSION type provider's diagnostics path.
//
// The in-process extension provider (`crates/verter_lsp/src/extension_provider.rs`,
// `provider_id = "extension"`) answers Verter's `$/verter/tsQuery` requests by
// dispatching them to this `ExtensionTsService`. Its `get_diagnostics` now pulls
// all THREE tsserver-family diagnostic passes — SEMANTIC, SYNTACTIC, and
// SUGGESTION — and unions them through the shared `merge_diagnostic_sets` owner,
// reaching parity with the native TS experience (and with TSGO's pull model).
// This requires the matching `syntacticDiagnosticsSync` and
// `suggestionDiagnosticsSync` command handlers on the TS side.
//
// This test drives `ExtensionTsService.handleQuery` directly (headless — NO VS
// Code, NO LSP) and asserts each diagnostic command returns the expected shape.
// Before the GAP-2 handlers existed, `handleQuery("suggestionDiagnosticsSync")`
// and `handleQuery("syntacticDiagnosticsSync")` threw `Unknown command:` — so
// these assertions are discriminating (reverting the handlers makes them throw).

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { ExtensionTsService } from "./extensionTsService.js";

interface WireDiagnostic {
  start?: { line: number; offset: number };
  end?: { line: number; offset: number };
  text: string;
  code: number;
  category: "error" | "warning" | "suggestion";
}

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function makeProject(fileName: string, source: string): { root: string; filePath: string } {
  const root = mkdtempSync(join(tmpdir(), "ext-diag-"));
  tmps.push(root);
  writeFileSync(
    join(root, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: {
        module: "esnext",
        target: "esnext",
        moduleResolution: "bundler",
        strict: true,
        // NOTE: `noUnusedLocals` is deliberately OFF — with it ON, an unused
        // local is a SEMANTIC error (6133); OFF, the unused-symbol finding flows
        // through the SUGGESTION pass (`getSuggestionDiagnostics`), which is what
        // the GAP-2 suggestion-merge assertion exercises.
      },
      include: ["*.ts", "*.tsx"],
    }),
  );
  const filePath = join(root, fileName);
  writeFileSync(filePath, source);
  return { root, filePath };
}

describe("ExtensionTsService — diagnostics parity (semantic + syntactic + suggestion)", () => {
  it("returns the semantic type error via semanticDiagnosticsSync", () => {
    const source = 'export const broken: number = "not a number";\n';
    const { root, filePath } = makeProject("entry.ts", source);
    const svc = new ExtensionTsService(root);
    svc.handleQuery("open", {
      file: filePath,
      fileContent: source,
      scriptKindName: "TS",
      projectRootPath: root,
    });

    const diags = svc.handleQuery("semanticDiagnosticsSync", {
      file: filePath,
    }) as WireDiagnostic[];
    expect(Array.isArray(diags)).toBe(true);
    const typeError = diags.find((d) => d.code === 2322 || /not assignable/.test(d.text));
    expect(typeError, "the assignability error (2322) must be present").toBeDefined();
    expect(typeError!.category).toBe("error");
  });

  it("returns the unused-symbol suggestion via suggestionDiagnosticsSync (GAP-2)", () => {
    // A locally-declared symbol that is never read → a SUGGESTION diagnostic.
    const source = "function f() {\n  const neverRead = 42;\n  return 1;\n}\nexport { f };\n";
    const { root, filePath } = makeProject("entry.ts", source);
    const svc = new ExtensionTsService(root);
    svc.handleQuery("open", {
      file: filePath,
      fileContent: source,
      scriptKindName: "TS",
      projectRootPath: root,
    });

    // Before the GAP-2 handler existed this threw `Unknown command:` — its mere
    // success is part of the discriminating signal.
    const diags = svc.handleQuery("suggestionDiagnosticsSync", {
      file: filePath,
    }) as WireDiagnostic[];
    expect(Array.isArray(diags), "suggestionDiagnosticsSync must be handled (GAP-2)").toBe(true);
    const unused = diags.find(
      (d) => /never read|never used|declared but/.test(d.text) || d.code === 6133,
    );
    expect(
      unused,
      `the unused-symbol suggestion must be present, got: ${JSON.stringify(diags)}`,
    ).toBeDefined();
  });

  it("returns a parse error via syntacticDiagnosticsSync (GAP-2)", () => {
    // A missing closing brace → a SYNTACTIC parse error.
    const source = "export function broken() {\n  return 1;\n";
    const { root, filePath } = makeProject("entry.ts", source);
    const svc = new ExtensionTsService(root);
    svc.handleQuery("open", {
      file: filePath,
      fileContent: source,
      scriptKindName: "TS",
      projectRootPath: root,
    });

    const diags = svc.handleQuery("syntacticDiagnosticsSync", {
      file: filePath,
    }) as WireDiagnostic[];
    expect(Array.isArray(diags), "syntacticDiagnosticsSync must be handled (GAP-2)").toBe(true);
    expect(
      diags.length,
      `a parse error must produce at least one syntactic diagnostic, got: ${JSON.stringify(diags)}`,
    ).toBeGreaterThan(0);
    expect(diags.every((d) => d.category === "error")).toBe(true);
  });

  it("maps every diagnostic to the {start,end,text,code,category} wire shape", () => {
    const source = 'export const broken: number = "x";\n';
    const { root, filePath } = makeProject("entry.ts", source);
    const svc = new ExtensionTsService(root);
    svc.handleQuery("open", {
      file: filePath,
      fileContent: source,
      scriptKindName: "TS",
      projectRootPath: root,
    });

    const diags = svc.handleQuery("semanticDiagnosticsSync", {
      file: filePath,
    }) as WireDiagnostic[];
    expect(diags.length).toBeGreaterThan(0);
    for (const d of diags) {
      expect(typeof d.text).toBe("string");
      expect(typeof d.code).toBe("number");
      expect(["error", "warning", "suggestion"]).toContain(d.category);
      // A located diagnostic carries 1-based line/offset positions.
      if (d.start) {
        expect(d.start.line).toBeGreaterThanOrEqual(1);
        expect(d.start.offset).toBeGreaterThanOrEqual(1);
      }
    }
  });
});
