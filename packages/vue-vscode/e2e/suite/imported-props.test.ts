import { expect } from "chai";
import { sequenceParent, waitForCompletionsMatching, waitForHoverMatching } from "../helpers";
import * as vscode from "vscode";
import {
  assertLogNotContains,
  findPosition,
  FIXTURE_NAME,
  getCompletions,
  measureHover,
  openVueFile,
  sleep,
  TYPE_PROVIDER,
  ensureFixtureWarm,
} from "../helpers";

suite(`Imported Props [${FIXTURE_NAME}]`, function () {
  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    if (FIXTURE_NAME !== "single-project" || !TYPE_PROVIDER) {
      this.skip();
      return;
    }

    await ensureFixtureWarm();
    doc = await openVueFile("src/ImportedPropsComp.vue");
  });

  // @ai-generated - Locks the opened-.vue imported-props regression from VS Code.
  test("hover and completions work for imported defineProps with withDefaults", async function () {
    // A hover AND a completion, in series on the same cold document, so the test
    // carries a deadline that can hold both (`POLL_SEQUENCES`). Two 20s loops
    // under the default 15s deadline meant neither could ever reach its own.
    this.timeout(sequenceParent("importedPropsHoverThenCompletion"));
    const hoverPos = findPosition(doc, "{{ title }}", 3);
    expect(hoverPos, "should find title usage").to.exist;

    const hovers = await waitForHoverMatching(doc.uri, hoverPos!, {
      predicate: (candidates) => candidates.length > 0 && candidates[0].contents.length > 0,
    });
    expect(hovers.length, "hover should resolve on imported prop").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "hover should have content").to.be.greaterThan(0);

    const completionPos = findPosition(doc, "props.cou", "props.".length);
    expect(completionPos, "should find partial imported prop member usage").to.exist;

    const isTypedCount = (item: vscode.CompletionItem | undefined): boolean =>
      item !== undefined && item.kind !== undefined && item.kind !== vscode.CompletionItemKind.Text;
    const completions = await waitForCompletionsMatching(doc.uri, completionPos!, {
      predicate: (list) => isTypedCount(list?.items.find((item) => item.label === "count")),
    });
    const countCompletion = completions?.items.find((item) => item.label === "count");

    expect(completions, "should return completions").to.exist;
    expect(countCompletion, "should include the imported count prop").to.exist;
    expect(
      countCompletion!.kind,
      "imported props member completion should be typed, not plain text",
    ).to.not.equal(vscode.CompletionItemKind.Text);

    assertLogNotContains(
      "panicked at",
      "imported prop hover/completion should not trigger Rust panics",
    );
  });
});
