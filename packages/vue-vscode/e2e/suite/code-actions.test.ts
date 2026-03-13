import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openAndReady,
  getCodeActions,
  FIXTURE_NAME,
  TYPE_PROVIDER,
  waitForCodeActionsMatching,
} from "../helpers";

function isCodeAction(item: vscode.CodeAction | vscode.Command): item is vscode.CodeAction {
  return "kind" in item;
}

suite(`Code Actions [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady();
    doc = await openAndReady("src/OrganizeImports.vue");
  });

  test("organize imports action available with source.organizeImports filter", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    // Request code actions for the import region after the type provider has settled.
    const importRange = new vscode.Range(
      new vscode.Position(1, 0),
      new vscode.Position(3, 0),
    );
    const actions = await waitForCodeActionsMatching(doc.uri, importRange, {
      kind: vscode.CodeActionKind.SourceOrganizeImports,
      predicate: (items: readonly (vscode.CodeAction | vscode.Command)[]) =>
        items.some(
          (item: vscode.CodeAction | vscode.Command) =>
            isCodeAction(item) &&
            item.kind?.value?.startsWith("source.organizeImports"),
        ),
      stableMs: 500,
      timeoutMs: 20_000,
      intervalMs: 150,
    });

    const filteredActions = await getCodeActions(
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
    const quickfixActions = filteredActions.filter(
      (a: vscode.CodeAction | vscode.Command) =>
        isCodeAction(a) && a.kind?.value?.startsWith("quickfix"),
    );
    expect(
      quickfixActions.length,
      "should not have quickfix actions when filtering by organizeImports",
    ).to.equal(0);
  });

  test("unfiltered request returns multiple action kinds", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    const scriptRange = new vscode.Range(
      new vscode.Position(0, 0),
      new vscode.Position(10, 0),
    );
    const actions = await waitForCodeActionsMatching(doc.uri, scriptRange, {
      predicate: (items: readonly (vscode.CodeAction | vscode.Command)[]) => {
        const kinds = new Set(
          items
            .filter(isCodeAction)
            .map((item: vscode.CodeAction) => item.kind?.value.split(".")[0])
            .filter((kind: string | undefined): kind is string => Boolean(kind)),
        );
        return kinds.has("source") && kinds.has("quickfix");
      },
      stableMs: 500,
      timeoutMs: 20_000,
      intervalMs: 150,
    });

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
