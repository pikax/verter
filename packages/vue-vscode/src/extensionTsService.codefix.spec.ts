// Headless regression guard for the EXTENSION type provider's unused-declaration
// quick-fix bridge plumbing (ISSUE-8, TS6133).
//
// The in-process extension provider (`crates/verter_lsp/src/extension_provider.rs`,
// `provider_id = "extension"`) requests TypeScript's "Remove unused declaration"
// fix via `getCodeFixes`, reads `fix.fixId` / `fix.fixAllDescription` off the
// response, and — per distinct typed `fixId` — sends the SHARED
// `combined_code_fix_args` scope shape to `getCombinedCodeFix`
// (`{ scope: { type: "file", args: { file } }, fixId }`) for the "Delete all unused
// declarations" companion.
//
// This characterizes the BRIDGE-PLUMBING contract the Rust side depends on, NOT
// TypeScript's fix-all behaviour (whether `ts.getCodeFixesAtPosition` attaches a
// `fixId` for a given unused decl, and whether `ts.getCombinedCodeFix` can produce
// a fix-all for it, is a property of the live tsserver process — covered by the
// real-provider e2e in `crates/verter_lsp/src/real_provider_tests/code_action.rs`).
//
// Two pre-fix defects this guards:
//   1. `getCodeFixes` mapped each fix to ONLY `{ description, changes }`, DROPPING
//      `fixId` + `fixAllDescription` — so the Rust side could never identify a
//      combinable fix. The keys must now be PRESENT on every forwarded fix.
//   2. There was NO `getCombinedCodeFix` case — the bridge dispatcher threw
//      `Unknown command: getCombinedCodeFix`. The case must now exist (it
//      dispatches to `ts.getCombinedCodeFix`; it never reports "Unknown command").
//
// Drives `ExtensionTsService.handleQuery` directly (headless — NO VS Code, NO LSP).

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { ExtensionTsService } from "./extensionTsService.js";

interface CodeFix {
  description: string;
  changes: unknown[];
}

const TS6133 = 6133; // "declared but its value is never read"

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function openUnusedFixture() {
  const root = mkdtempSync(join(tmpdir(), "ext-unused-"));
  tmps.push(root);
  writeFileSync(
    join(root, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: {
        module: "esnext",
        target: "esnext",
        moduleResolution: "bundler",
        noUnusedLocals: true,
      },
      include: ["*.ts"],
    }),
  );
  const filePath = join(root, "only.ts");
  // `unusedA` on line 1 is never referenced → a TS6133 "Remove unused
  // declaration" fix is offered over its declaration.
  const source = "const unusedA = 1;\nconst unusedB = 2;\n";
  writeFileSync(filePath, source);

  const svc = new ExtensionTsService(root);
  svc.handleQuery("open", {
    file: filePath,
    fileContent: source,
    scriptKindName: "TS",
    projectRootPath: root,
  });
  return { svc, filePath };
}

describe("ExtensionTsService — unused-declaration code-fix bridge plumbing", () => {
  it("getCodeFixes forwards the typed fixId + fixAllDescription keys (never drops them)", () => {
    const { svc, filePath } = openUnusedFixture();

    // `getCodeFixes` over the `unusedA` identifier (line 1, cols 7..14) with the
    // TS6133 error code — exactly the request the Rust provider issues.
    const fixes = svc.handleQuery("getCodeFixes", {
      file: filePath,
      startLine: 1,
      startOffset: 7,
      endLine: 1,
      endOffset: 14,
      errorCodes: [TS6133],
    }) as CodeFix[];

    expect(Array.isArray(fixes), "getCodeFixes returns an array").toBe(true);
    expect(fixes.length, "TypeScript offers a remove-unused fix").toBeGreaterThan(0);

    // The discriminating plumbing assertion: every forwarded fix must CARRY the
    // `fixId` and `fixAllDescription` keys. The pre-fix bridge omitted them from
    // the mapped object entirely (`Object.keys(fix)` was just
    // `["description","changes"]`), so the Rust `fix.get("fixId")` was always None.
    // The values may be `undefined` for a non-combinable fix — what matters is the
    // KEY is present so a combinable fix's id round-trips.
    for (const fix of fixes) {
      expect(
        Object.prototype.hasOwnProperty.call(fix, "fixId"),
        `getCodeFixes must forward the fixId key (got keys ${JSON.stringify(Object.keys(fix))})`,
      ).toBe(true);
      expect(
        Object.prototype.hasOwnProperty.call(fix, "fixAllDescription"),
        "getCodeFixes must forward the fixAllDescription key",
      ).toBe(true);
    }
  });

  it("getCombinedCodeFix is a dispatched command (no 'Unknown command') reading the shared scope shape", () => {
    const { svc, filePath } = openUnusedFixture();

    // The Rust side sends the shared `combined_code_fix_args` scope shape. Before
    // the fix there was no `case "getCombinedCodeFix"`, so the dispatcher threw
    // `Unknown command: getCombinedCodeFix`. After the fix the case exists and
    // delegates to `ts.getCombinedCodeFix({ type: "file", fileName }, fixId, …)`.
    //
    // (Whether the underlying TS engine can synthesize a fix-all for a given fixId
    // is environment-dependent and is NOT what this guards — the real-provider e2e
    // covers the live fix-all. Here we assert ONLY that the command is recognised
    // and reads the scope shape, never the "Unknown command" dispatch error.)
    let err: string | undefined;
    try {
      svc.handleQuery("getCombinedCodeFix", {
        scope: { type: "file", args: { file: filePath } },
        fixId: "unusedIdentifier",
      });
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    }

    expect(
      err === undefined || !/unknown command/i.test(err),
      `getCombinedCodeFix must be a recognised command, not fall through to the \
"Unknown command" default (got error: ${err ?? "<none>"})`,
    ).toBe(true);

    // A bogus command still hits the default — proving the dispatcher's default
    // arm is intact and the assertion above is meaningful (the case, not a
    // swallowed default, handled getCombinedCodeFix).
    let bogusErr: string | undefined;
    try {
      svc.handleQuery("definitelyNotACommand", { file: filePath });
    } catch (e) {
      bogusErr = e instanceof Error ? e.message : String(e);
    }
    expect(bogusErr, "an unknown command must still throw Unknown command").toMatch(
      /unknown command/i,
    );
  });
});
