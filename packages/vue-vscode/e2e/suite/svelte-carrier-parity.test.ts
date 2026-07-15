import { expect } from "chai";
import * as vscode from "vscode";
import {
  ensureTypeProviderSynced,
  isLspReady,
  openVueFile,
  waitForFileReady,
  getCompletions,
  FIXTURE_NAME,
} from "../helpers";

// Block S2b: server-side `.svelte`-carrier LSP feature parity with Vue.
//
// The `.svelte` carrier reaches the SAME LSP features as `.vue` through the
// shared carrier-generic substrate (no Vue fork). These tests open the
// committed `SvelteParent.svelte` / `SvelteChild.svelte` fixtures and assert
// each de-Vue-gated feature returns correct non-empty results for a `.svelte`
// carrier.
//
// The Svelte fixtures live only in the `single-project` fixture; for every
// other fixture these tests pass with an N/A note (per the e2e skill's
// "return early instead of skip" convention).

const SVELTE_PARENT = "src/SvelteParent.svelte";
const SVELTE_CHILD = "src/SvelteChild.svelte";

function symbolNames(symbols: vscode.SymbolInformation[]): string[] {
  return symbols.map((s) => s.name);
}

suite(`Svelte carrier parity [${FIXTURE_NAME}]`, function () {
  let parentDoc: vscode.TextDocument;

  suiteSetup(async function () {
    expect(isLspReady(), "LSP must be ready").to.be.true;
    if (FIXTURE_NAME !== "single-project") return;
    await ensureTypeProviderSynced();
    parentDoc = await openVueFile(SVELTE_PARENT);
    await waitForFileReady(parentDoc);
    const childDoc = await openVueFile(SVELTE_CHILD);
    await waitForFileReady(childDoc);
    // Re-focus the parent for the per-test cursor lookups.
    parentDoc = await openVueFile(SVELTE_PARENT);
  });

  // Workspace symbols include `.svelte` components.
  test("workspace-symbol search finds a Svelte component's symbols", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    svelte fixtures only in single-project — N/A");
      return;
    }
    const symbols =
      (await vscode.commands.executeCommand<vscode.SymbolInformation[]>(
        "vscode.executeWorkspaceSymbolProvider",
        "svelteParent",
      )) || [];
    const fromSvelte = symbols.filter((s) => s.location.uri.fsPath.endsWith(".svelte"));
    console.log(`    workspace symbols from .svelte: ${symbolNames(fromSvelte).join(", ")}`);
    expect(
      fromSvelte.length,
      "a .svelte carrier must contribute workspace symbols",
    ).to.be.greaterThan(0);
  });

  // Component auto-import offers a Svelte component with the correct
  // (extension-stripped) name.
  test("completion offers a Svelte component auto-import with the stripped name", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    svelte fixtures only in single-project — N/A");
      return;
    }
    // Type a `<Sv` prefix inside the parent template and request completions.
    const text = parentDoc.getText();
    const anchor = text.indexOf("<SvelteChild");
    expect(anchor, "fixture must contain a <SvelteChild usage").to.be.greaterThan(-1);
    const pos = parentDoc.positionAt(anchor + 1); // just after `<`
    const list = await getCompletions(parentDoc.uri, pos);
    const labels = (list?.items || []).map((i) =>
      typeof i.label === "string" ? i.label : i.label.label,
    );
    console.log(`    completion labels (sample): ${labels.slice(0, 20).join(", ")}`);
    expect(
      labels.some((l) => l === "SvelteChild"),
      "completion must offer the Svelte component by its stripped PascalCase name",
    ).to.be.true;
  });

  // Definition on a Svelte child usage lands on the `.svelte` child.
  test("definition on a Svelte child usage lands on the .svelte component", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    svelte fixtures only in single-project — N/A");
      return;
    }
    const text = parentDoc.getText();
    // The `SvelteChild` identifier in the import statement.
    const importIdx = text.indexOf("import SvelteChild");
    expect(importIdx, "fixture must import SvelteChild").to.be.greaterThan(-1);
    const pos = parentDoc.positionAt(importIdx + "import ".length + 1);
    const locations =
      (await vscode.commands.executeCommand<vscode.Location[]>(
        "vscode.executeDefinitionProvider",
        parentDoc.uri,
        pos,
      )) || [];
    console.log(`    definition locations: ${locations.map((l) => l.uri.fsPath).join(", ")}`);
    expect(
      locations.some((l) => l.uri.fsPath.endsWith("SvelteChild.svelte")),
      "definition must land on the .svelte child component",
    ).to.be.true;
  });

  // The Svelte carrier opens and the type provider processes it without error
  // (the carrier-generic open/ready path applies to `.svelte`).
  test("a .svelte carrier opens and reaches type-provider readiness", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    svelte fixtures only in single-project — N/A");
      return;
    }
    const childDoc = await openVueFile(SVELTE_CHILD);
    await waitForFileReady(childDoc);
    expect(childDoc.languageId).to.equal("svelte");
  });
});
