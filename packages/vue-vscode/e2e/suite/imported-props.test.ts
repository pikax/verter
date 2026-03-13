import { expect } from "chai";
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
  waitForExtensionReady,
} from "../helpers";

suite(`Imported Props [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    if (FIXTURE_NAME !== "single-project" || !TYPE_PROVIDER) {
      this.skip();
      return;
    }

    await waitForExtensionReady();
    doc = await openVueFile("src/ImportedPropsComp.vue");
  });

  // @ai-generated - Locks the opened-.vue imported-props regression from VS Code.
  test("hover and completions work for imported defineProps with withDefaults", async function () {
    const hoverPos = findPosition(doc, "{{ title }}", 3);
    expect(hoverPos, "should find title usage").to.exist;

    let hovers: vscode.Hover[] = [];
    const hoverDeadline = Date.now() + 20_000;
    while (Date.now() < hoverDeadline) {
      ({ hovers } = await measureHover(doc.uri, hoverPos!));
      if (hovers.length > 0 && hovers[0].contents.length > 0) {
        break;
      }
      await sleep(200);
    }
    expect(hovers.length, "hover should resolve on imported prop").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "hover should have content").to.be.greaterThan(0);

    const completionPos = findPosition(doc, "props.cou", "props.".length);
    expect(completionPos, "should find partial imported prop member usage").to.exist;

    let completions: vscode.CompletionList | undefined;
    let countCompletion: vscode.CompletionItem | undefined;
    const completionDeadline = Date.now() + 20_000;
    while (Date.now() < completionDeadline) {
      completions = await getCompletions(doc.uri, completionPos!);
      countCompletion = completions?.items.find((item) => item.label === "count");
      if (
        countCompletion &&
        countCompletion.kind !== undefined &&
        countCompletion.kind !== vscode.CompletionItemKind.Text
      ) {
        break;
      }
      await sleep(200);
    }

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
