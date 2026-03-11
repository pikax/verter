import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openAndReady,
  getAppVuePath,
  getCodeActions,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";

function isCodeAction(item: vscode.CodeAction | vscode.Command): item is vscode.CodeAction {
  return "kind" in item;
}

suite(`Code Actions [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady();
    doc = await openAndReady(getAppVuePath());
  });

  test("organize imports action available with source.organizeImports filter", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    // Request code actions for the import region (lines 1-12, 0-indexed: 0-11)
    const importRange = new vscode.Range(
      new vscode.Position(1, 0),
      new vscode.Position(11, 0),
    );
    const actions = await getCodeActions(
      doc.uri,
      importRange,
      vscode.CodeActionKind.SourceOrganizeImports,
    );

    // Should have at least one organize imports action
    const organizeActions = actions.filter(
      (a) => isCodeAction(a) && a.kind?.value?.startsWith("source.organizeImports"),
    );
    expect(
      organizeActions.length,
      "should have at least one organize imports action",
    ).to.be.greaterThan(0);

    // Should NOT have quickfix or refactor actions when filtering by source.organizeImports
    const quickfixActions = actions.filter(
      (a) => isCodeAction(a) && a.kind?.value?.startsWith("quickfix"),
    );
    expect(
      quickfixActions.length,
      "should not have quickfix actions when filtering by organizeImports",
    ).to.equal(0);
  });

  test("unfiltered request returns multiple action kinds", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    // Request code actions for the full script range without kind filter
    const scriptRange = new vscode.Range(
      new vscode.Position(0, 0),
      new vscode.Position(32, 0),
    );
    const actions = await getCodeActions(doc.uri, scriptRange);

    // Should have at least some actions
    expect(actions.length, "should have code actions").to.be.greaterThan(0);

    // Collect all unique kind prefixes
    const kindPrefixes = new Set<string>();
    for (const a of actions) {
      if (isCodeAction(a) && a.kind) {
        const prefix = a.kind.value.split(".")[0];
        kindPrefixes.add(prefix);
      }
    }

    // Should have at least "source" (organize imports)
    expect(
      kindPrefixes.has("source"),
      `should have source actions, got kinds: [${[...kindPrefixes].join(", ")}]`,
    ).to.be.true;
  });
});
