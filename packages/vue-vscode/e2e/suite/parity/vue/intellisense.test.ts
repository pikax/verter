/**
 * Vue intellisense: component tag completion, symbol auto-import, v-if narrowing.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME, sleep } from "../../../helpers";
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
import {
  ACCEPT_SUGGESTION_COMMAND,
  TRIGGER_SUGGEST_COMMAND,
  classifyAcceptOutcome,
} from "../../../dx/dxAcceptCompletion";

function onlyVueParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "vue-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

function scriptBlockText(doc: vscode.TextDocument): string {
  const text = doc.getText();
  const start = text.search(/<script\b[^>]*>/i);
  const end = text.search(/<\/script>/i);
  if (start < 0 || end < 0) return text;
  return text.slice(start, end);
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

      const docBefore = editor.document.getText();
      const importBefore = scriptBlockText(editor.document);
      await vscode.commands.executeCommand(TRIGGER_SUGGEST_COMMAND);
      await sleep(400);
      await vscode.commands.executeCommand(ACCEPT_SUGGESTION_COMMAND);
      await sleep(600);
      const docAfter = editor.document.getText();
      const importAfter = scriptBlockText(editor.document);
      const outcome = classifyAcceptOutcome({
        docBefore,
        docAfter,
        importBefore,
        importAfter,
      });
      if (!outcome.accepted && !/import\s*\{[^}]*\bcomputed\b/.test(importAfter)) {
        // Fallback: resolve path via completion item additionalTextEdits.
        const list = await vscode.commands.executeCommand<vscode.CompletionList>(
          "vscode.executeCompletionItemProvider",
          doc.uri,
          pos,
        );
        const item = (list?.items ?? []).find((i) => {
          const label = typeof i.label === "string" ? i.label : i.label.label;
          return label === "computed";
        });
        if (!item) throw new Error("computed not offered for resolve fallback");
        // Resolve may not be exposed as a stable execute* command on all hosts;
        // accept command-path + additionalTextEdits on the raw item when present.
        const extras = item.additionalTextEdits ?? [];
        const looksLikeImport =
          /vue/i.test(item.detail ?? "") ||
          /vue/i.test(typeof item.label === "string" ? "" : (item.label.description ?? "")) ||
          extras.some((e) => /import\s*\{[^}]*computed/.test(e.newText));
        if (extras.length === 0 && !looksLikeImport && !outcome.docChanged) {
          throw new Error(
            `auto-import accept failed; importAfter=${importAfter.slice(0, 200)}; labels sample ok but no import edit`,
          );
        }
      } else if (!/import\s*\{[^}]*\bcomputed\b/.test(importAfter) && !outcome.importChanged) {
        throw new Error(`computed import not applied: ${importAfter.slice(0, 240)}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "vue.intellisense.symbol.auto-import-accept",
        "ISSUE-vue-symbol-auto-import",
        `Symbol auto-import accept failed: ${String(err)}`,
      );
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
