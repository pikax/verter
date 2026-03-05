import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  sleep,
  FIXTURE_NAME,
} from "../helpers";

suite(`Definition [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady(60_000);
    doc = await openVueFile(getAppVuePath());
    // Wait for LSP to fully process the file
    await sleep(8_000);
  });

  test("go-to-definition on prop binding in template", async function () {
    // Template has: <h1>{{ title }}</h1>
    // Script has: const props = defineProps<{ title: string }>()
    const text = doc.getText();
    const templateMatch = text.indexOf("{{ title }}");
    if (templateMatch === -1) {
      console.log("    {{ title }} not in fixture — skip");
      this.skip();
      return;
    }

    // Position on "title" inside {{ title }}
    const pos = doc.positionAt(templateMatch + 3);

    const locations = await vscode.commands.executeCommand<vscode.Location[]>(
      "vscode.executeDefinitionProvider",
      doc.uri,
      pos,
    );

    console.log(`    Definition on title: ${locations?.length ?? 0} location(s)`);

    expect(locations, "should return definition locations").to.exist;
    expect(locations!.length, "should have at least 1 definition").to.be.greaterThan(0);

    // The definition should point to "title" in the defineProps type parameter
    const def = locations![0];
    const definePropsLine = text.indexOf("defineProps<{ title: string }>");
    expect(definePropsLine, "fixture should have defineProps").to.not.equal(-1);

    // The definition should be in the same file
    expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

    // The definition should point to the "title" property key in the type parameter
    const titleInDefineProps = text.indexOf("title", definePropsLine);
    const expectedPos = doc.positionAt(titleInDefineProps);
    expect(def.range.start.line, "definition should be on the defineProps line").to.equal(
      expectedPos.line,
    );
    expect(def.range.start.character, "definition should point to 'title' in type param").to.equal(
      expectedPos.character,
    );
  });

  test("go-to-definition on ref binding in template", async function () {
    const text = doc.getText();
    const templateMatch = text.indexOf("{{ count }}");
    if (templateMatch === -1) {
      console.log("    {{ count }} not in fixture — skip");
      this.skip();
      return;
    }

    // Position on "count" inside {{ count }}
    const pos = doc.positionAt(templateMatch + 3);

    const locations = await vscode.commands.executeCommand<vscode.Location[]>(
      "vscode.executeDefinitionProvider",
      doc.uri,
      pos,
    );

    console.log(`    Definition on count: ${locations?.length ?? 0} location(s)`);

    expect(locations, "should return definition locations").to.exist;
    expect(locations!.length, "should have at least 1 definition").to.be.greaterThan(0);

    // The definition should be in the same file
    const def = locations![0];
    expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

    // Should point to "count" in "const count = ref(0)"
    const countDecl = text.indexOf("const count = ref(0)");
    if (countDecl !== -1) {
      const expectedLine = doc.positionAt(countDecl).line;
      expect(def.range.start.line, "definition should be on the ref declaration line").to.equal(
        expectedLine,
      );
    }
  });
});
