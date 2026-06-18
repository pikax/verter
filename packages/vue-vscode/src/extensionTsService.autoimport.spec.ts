// Headless regression guard for the EXTENSION type provider's auto-import path.
//
// The in-process extension provider (`crates/verter_lsp/src/extension_provider.rs`,
// `provider_id = "extension"`) answers Verter's `$/verter/tsQuery` requests by
// dispatching them to this `ExtensionTsService` in the VS Code extension host.
// Its Rust-side parse/offset-stamp/envelope/resolve-routing reuses the EXACT
// shared tsserver-family code path as `TsserverTypeProvider` (verified: it calls
// `parse_tsserver_completion`, `stamp_tsserver_completion_offset`,
// `build_completion_entry_details_request`, `build_entry_names_entry`,
// `completion_entry_details_to_resolve_result`), which the real-tsserver
// `providerResolveParity.integration.test.ts` gate already proves.
//
// The ONLY extension-specific surface is the TS-side response shaping done HERE:
// `completionInfo` must surface an unimported module export as an entry carrying
// `source`, and `completionEntryDetails` must return the auto-import `codeActions`
// the shared Rust mapper turns into an import edit. This test drives
// `ExtensionTsService.handleQuery` directly (headless — NO VS Code, NO LSP) over
// the SAME tiny two-file workspace the DX parity gate uses, proving the
// extension provider produces a real auto-import for an unimported symbol.
//
// Why not the DX parity gate or the VS Code E2E job?
//   * The DX bridge only spawns child-process providers (tsgo / tsserver); the
//     extension provider needs a live extension-host LSP `Client`, so it cannot
//     be driven headlessly there without building a whole new bridge mode.
//   * The `vscode-e2e` CI job is deliberately `if: false` (flaky); re-enabling it
//     would revive the instability the parity block was built to replace.
// codex-ratified (xhigh): drive `ExtensionTsService` directly + lean on the
// shared-Rust-path tsserver parity for the rest. See
// `docs/arch/provider-completion-resolve-design.md`.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { ExtensionTsService } from "./extensionTsService.js";

/** A bound import the resolved edit must produce: `import { myHelper } from "./helper"`. */
const EXPECT_IMPORT_SYMBOL = "myHelper";
const EXPECT_IMPORT_MODULE = "./helper";

interface CompletionEntry {
  name: string;
  source?: string;
  data?: unknown;
  hasAction?: boolean;
}
interface TextChange {
  newText: string;
}
interface CodeActionChange {
  fileName: string;
  textChanges: TextChange[];
}
interface CodeAction {
  description: string;
  changes: CodeActionChange[];
}
interface EntryDetail {
  name: string;
  codeActions?: CodeAction[];
}

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

describe("ExtensionTsService — extension provider auto-import shaping", () => {
  it("surfaces an unimported export with a `source` resolve key and resolves an import edit", () => {
    // A tiny two-file workspace: `entry.ts` references an UNIMPORTED `myHelper`
    // exported by `helper.ts`. A tsconfig makes them one program so the
    // language service offers `myHelper` as an auto-import candidate.
    const root = mkdtempSync(join(tmpdir(), "ext-autoimport-"));
    tmps.push(root);
    writeFileSync(
      join(root, "tsconfig.json"),
      JSON.stringify({
        compilerOptions: { module: "esnext", target: "esnext", moduleResolution: "bundler" },
        include: ["*.ts"],
      }),
    );
    const helperPath = join(root, "helper.ts");
    const entryPath = join(root, "entry.ts");
    const helperSource = "export const myHelper = 41;\n";
    const entrySource = "myHelper\n"; // `myHelper` referenced but NOT imported.
    writeFileSync(helperPath, helperSource);
    writeFileSync(entryPath, entrySource);

    const svc = new ExtensionTsService(root);

    // Open both files (the extension provider sends `open` per file).
    svc.handleQuery("open", {
      file: entryPath,
      fileContent: entrySource,
      scriptKindName: "TS",
      projectRootPath: root,
    });
    svc.handleQuery("open", {
      file: helperPath,
      fileContent: helperSource,
      scriptKindName: "TS",
      projectRootPath: root,
    });

    // Completion at the END of `myHelper` on line 1 (1-based tsserver position:
    // byte offset 8 -> col 9). The provider asks `completionInfo` with the same
    // include-external-module-exports flags the Rust `get_completions` sets.
    const completion = svc.handleQuery("completionInfo", {
      file: entryPath,
      line: 1,
      offset: EXPECT_IMPORT_SYMBOL.length + 1,
      includeExternalModuleExports: true,
      includeInsertTextCompletions: true,
    }) as { entries: CompletionEntry[] } | undefined;

    expect(completion, "completionInfo must return a result").toBeDefined();
    const entry = completion!.entries.find((e) => e.name === EXPECT_IMPORT_SYMBOL && e.source);
    // The auto-import entry MUST carry `source` (the module specifier) — the
    // resolve key the shared Rust `is_actionable` / `TsserverEntry` rail needs.
    // Without `source` the extension provider could never resolve an import.
    expect(
      entry,
      "completionInfo must surface the unimported `myHelper` as an entry carrying a `source` (the auto-import resolve key)",
    ).toBeDefined();
    expect(entry!.source).toBeTruthy();

    // Resolve the entry through `completionEntryDetails` at the SAME position,
    // forwarding `name`/`source`/`data` exactly as the Rust
    // `build_entry_names_entry` builder does.
    const details = svc.handleQuery("completionEntryDetails", {
      file: entryPath,
      line: 1,
      offset: EXPECT_IMPORT_SYMBOL.length + 1,
      entryNames: [{ name: entry!.name, source: entry!.source, data: entry!.data }],
    }) as EntryDetail[] | undefined;

    expect(Array.isArray(details), "completionEntryDetails must return an array").toBe(true);
    const detail = details!.find((d) => d.name === EXPECT_IMPORT_SYMBOL);
    expect(detail, "the resolved detail for `myHelper` must be present").toBeDefined();

    // The auto-import edit: a code action whose textChange inserts the import.
    const importText = (detail!.codeActions ?? [])
      .flatMap((a) => a.changes)
      .flatMap((c) => c.textChanges)
      .map((tc) => tc.newText)
      .join("");

    expect(
      detail!.codeActions && detail!.codeActions.length,
      "the resolved detail must carry an auto-import `codeAction` (the import edit set)",
    ).toBeGreaterThan(0);
    expect(importText, "the auto-import edit imports the symbol").toContain(EXPECT_IMPORT_SYMBOL);
    expect(importText, "the auto-import edit references the module").toContain(
      EXPECT_IMPORT_MODULE,
    );
  });

  it("does NOT offer an import edit for a local symbol (negative control)", () => {
    // A single file with a LOCAL `localThing` — completing it must NOT yield an
    // auto-import code action (it is already in scope, no `source`).
    const root = mkdtempSync(join(tmpdir(), "ext-autoimport-local-"));
    tmps.push(root);
    writeFileSync(
      join(root, "tsconfig.json"),
      JSON.stringify({
        compilerOptions: { module: "esnext", target: "esnext", moduleResolution: "bundler" },
        include: ["*.ts"],
      }),
    );
    const filePath = join(root, "only.ts");
    const source = "const localThing = 1;\nlocalThing\n";
    writeFileSync(filePath, source);

    const svc = new ExtensionTsService(root);
    svc.handleQuery("open", {
      file: filePath,
      fileContent: source,
      scriptKindName: "TS",
      projectRootPath: root,
    });

    // Completion at the END of `localThing` on line 2 (col = length + 1).
    const completion = svc.handleQuery("completionInfo", {
      file: filePath,
      line: 2,
      offset: "localThing".length + 1,
      includeExternalModuleExports: true,
      includeInsertTextCompletions: true,
    }) as { entries: CompletionEntry[] } | undefined;

    const localEntry = completion?.entries.find((e) => e.name === "localThing");
    expect(localEntry, "the local symbol must still be offered as a completion").toBeDefined();
    // The local entry must NOT carry an auto-import `source` — it is in scope.
    expect(
      localEntry!.source,
      "a local in-scope symbol must NOT carry an auto-import `source`",
    ).toBeFalsy();
  });
});
