/**
 * Vue intellisense: component tag completion, symbol auto-import, v-if narrowing.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertHoverLooksNarrowed,
  assertHoverNeedles,
  completionsAtOffset,
  ensureParityReady,
  findOffset,
  hoverTextAt,
  openRelative,
  pollUntil,
  failParityGap,
} from "../../../lib/parityHarness";
import { acceptCompletionInEditor } from "../../../dx/dxScenarioRunner";

function onlyVueParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "vue-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

/** Discard the accept's in-memory edits so later cases see the fixture as on disk. */
async function revertIfDirty(relative: string): Promise<void> {
  const open = vscode.workspace.textDocuments.find((d) =>
    d.uri.fsPath.replace(/\\/g, "/").endsWith(relative),
  );
  if (!open?.isDirty) return;
  await vscode.window.showTextDocument(open);
  await vscode.commands.executeCommand("workbench.action.files.revert");
}

suite(`Vue intellisense [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlyVueParity(this);
    await ensureParityReady("src/App.vue");
  });

  test("vue.intellisense.component-tag.completion", async function () {
    onlyVueParity(this);
    this.timeout(30_000);
    try {
      const doc = await openRelative("src/features/AutoImportTag.vue");
      // Insert incomplete tag at site marker (valid fixture; incomplete tag is edit-time only).
      const editor = await vscode.window.showTextDocument(doc);
      const marker = "<!-- TAG_COMPLETE_SITE -->";
      const at = findOffset(doc, marker) + marker.length;
      await editor.edit((eb) => eb.insert(doc.positionAt(at), "\n    <Prop"));
      const offset = findOffset(editor.document, "<Prop") + "<Prop".length;
      const labels = await completionsAtOffset("src/features/AutoImportTag.vue", offset);
      const hit = labels.some(
        (l) => l === "PropChild" || l.startsWith("PropChild") || l.includes("PropChild"),
      );
      if (!hit) {
        throw new Error(
          `expected PropChild in tag completions; sample=${labels.slice(0, 40).join(", ")}`,
        );
      }
    } catch (err) {
      failParityGap(
        this,
        "vue.intellisense.component-tag.completion",
        "ISSUE-vue-component-tag-completion",
        `Component tag completion (<Prop…) failed: ${String(err)}`,
      );
    }
  });

  test("vue.intellisense.symbol.auto-import-accept", async function () {
    onlyVueParity(this);
    this.timeout(60_000);
    try {
      const doc = await openRelative("src/features/AutoImportSymbol.vue");
      const editor = await vscode.window.showTextDocument(doc);
      const offset = findOffset(doc, "comput(() =>") + "comput".length;
      const pos = doc.positionAt(offset);
      editor.selection = new vscode.Selection(pos, pos);

      // Wait until `computed` is offered.
      await pollUntil(
        "computed auto-import offer",
        async () =>
          (await vscode.commands.executeCommand<vscode.CompletionList>(
            "vscode.executeCompletionItemProvider",
            doc.uri,
            pos,
          )) ?? { items: [] },
        (list) =>
          (list.items ?? []).some((item) => {
            const label = typeof item.label === "string" ? item.label : item.label.label;
            return label === "computed";
          }),
        20_000,
      );

      // One genuine editor accept: the shared driver waits for the suggestion widget
      // to focus `computed`, accepts it once, and passes only when the identifier
      // and its static import have both landed.
      const outcome = await acceptCompletionInEditor(editor, "computed");
      if (!/import\s*\{[^}]*\bcomputed\b/.test(outcome.importAfter)) {
        throw new Error(`computed import not applied: ${outcome.importAfter.slice(0, 240)}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "vue.intellisense.symbol.auto-import-accept",
        "ISSUE-vue-symbol-auto-import",
        `Symbol auto-import accept failed: ${String(err)}`,
      );
    } finally {
      await revertIfDirty("src/features/AutoImportSymbol.vue");
    }
  });

  test("vue.intellisense.v-if.narrowing.hover", async function () {
    onlyVueParity(this);
    try {
      // Inside v-if="person", person.name should be narrowed to Person (not null).
      const text = await hoverTextAt({
        file: "src/features/Narrowing.vue",
        token: "person",
        occurrence: 3, // first template use after setPerson/v-if/else — adjust if fails
      });
      assertHoverLooksNarrowed(text, ["name", "Person"]);
      if (/\bnull\b/.test(text) && !/Person/.test(text)) {
        throw new Error(`expected narrowed Person, got: ${text}`);
      }
    } catch (err) {
      // Retry on person.name member specifically.
      try {
        const text = await assertHoverNeedles(
          { file: "src/features/Narrowing.vue", token: "name", occurrence: 1 },
          ["string", "name"],
        );
        if (/:\s*any\b/.test(text)) throw new Error(text);
      } catch (inner) {
        failParityGap(
          this,
          "vue.intellisense.v-if.narrowing.hover",
          "ISSUE-vue-vif-narrowing-intellisense",
          `v-if narrowing hover failed: ${String(err)}; retry: ${String(inner)}`,
        );
      }
    }
  });

  test("vue.intellisense.v-if.narrowing.completion", async function () {
    onlyVueParity(this);
    try {
      const doc = await openRelative("src/features/Narrowing.vue");
      // After `person.` inside the v-if branch mustache.
      const needle = "person.name";
      const offset = findOffset(doc, needle) + "person.".length;
      const labels = await completionsAtOffset("src/features/Narrowing.vue", offset, ".");
      const hasName = labels.some((l) => l === "name" || l.startsWith("name"));
      const hasAge = labels.some((l) => l === "age" || l.startsWith("age"));
      if (!hasName || !hasAge) {
        throw new Error(
          `expected name/age on narrowed person; got ${labels.slice(0, 30).join(", ")}`,
        );
      }
    } catch (err) {
      failParityGap(
        this,
        "vue.intellisense.v-if.narrowing.completion",
        "ISSUE-vue-vif-narrowing-completion",
        `v-if member completion failed: ${String(err)}`,
      );
    }
  });
});
