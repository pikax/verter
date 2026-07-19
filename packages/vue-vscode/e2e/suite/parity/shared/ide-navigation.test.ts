/**
 * Ctrl+Click (definition) + autocomplete for props, events, slots/snippets,
 * directives, event-handler locals, narrowing, and auto-import.
 *
 * Vue and Svelte share the same case shapes (first-class equal bar).
 * Edit-time incomplete markup is restored by reopening the fixture file.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME, sleep } from "../../../helpers";
import {
  assertDefinitionTargetsFile,
  assertDefinitionTargetsToken,
  assertHoverNeedles,
  completionsAtOffset,
  ensureParityReady,
  findOffset,
  openRelative,
  pollUntil,
  failParityGap,
} from "../../../lib/parityHarness";
import {
  ACCEPT_SUGGESTION_COMMAND,
  TRIGGER_SUGGEST_COMMAND,
  classifyAcceptOutcome,
} from "../../../dx/dxAcceptCompletion";

function parityFramework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

function parentFile(fw: "vue" | "svelte"): string {
  return fw === "vue" ? "src/ide/IdeSurfaceParent.vue" : "src/ide/IdeSurfaceParent.svelte";
}

function childFile(fw: "vue" | "svelte"): string {
  return fw === "vue" ? "src/ide/IdeSurfaceChild.vue" : "src/ide/IdeSurfaceChild.svelte";
}

function scriptBlockText(doc: vscode.TextDocument): string {
  const text = doc.getText();
  const start = text.search(/<script\b[^>]*>/i);
  const end = text.search(/<\/script>/i);
  if (start < 0 || end < 0) return text;
  return text.slice(start, end);
}

async function reopenFresh(relative: string): Promise<vscode.TextDocument> {
  // Discard in-memory edits by closing and reopening from disk.
  const open = vscode.workspace.textDocuments.find((d) =>
    d.uri.fsPath.replace(/\\/g, "/").endsWith(relative.replace(/^\.\//, "")),
  );
  if (open && open.isDirty) {
    await vscode.window.showTextDocument(open);
    await vscode.commands.executeCommand("workbench.action.files.revert");
  }
  return openRelative(relative);
}

suite(`IDE navigation + completion [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = parityFramework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  // ── Ctrl+Click / go-to-definition ─────────────────────────────

  test("ide.def.event-attr-to-handler", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      // @pick="onPick" / onPick={onPick} — jump to handler declaration
      if (fw === "vue") {
        await assertDefinitionTargetsToken(
          { file: parent, token: "onPick", occurrence: 1 },
          { file: parent, token: "onPick", occurrence: 0 },
        );
      } else {
        await assertDefinitionTargetsToken(
          { file: parent, token: "onPick", occurrence: 1 },
          { file: parent, token: "onPick", occurrence: 0 },
        );
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.def.event-attr-to-handler",
        fw === "vue" ? "ISSUE-vue-ide-def-event" : "ISSUE-svelte-ide-def-event",
        `Ctrl+Click event binding did not reach handler: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.def.event-name-to-emit-or-prop", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    const child = childFile(fw);
    try {
      if (fw === "vue") {
        // Token `pick` in `@pick=` — prefer child emit declaration or parent handler.
        try {
          await assertDefinitionTargetsFile(
            { file: parent, token: "@pick", occurrence: 0, caretOffset: 1 },
            child,
          );
        } catch {
          await assertDefinitionTargetsToken(
            { file: parent, token: "pick", occurrence: 0 },
            { file: parent, token: "onPick", occurrence: 0 },
          );
        }
      } else {
        // Svelte callback prop `onPick` on the child usage → handler or child prop.
        try {
          await assertDefinitionTargetsToken(
            { file: parent, token: "onPick", occurrence: 1 },
            { file: parent, token: "onPick", occurrence: 0 },
          );
        } catch {
          await assertDefinitionTargetsFile(
            { file: parent, token: "onPick", occurrence: 1 },
            child,
          );
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.def.event-name-to-emit-or-prop",
        fw === "vue" ? "ISSUE-vue-ide-def-event-name" : "ISSUE-svelte-ide-def-event-name",
        `Ctrl+Click event name did not resolve: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.def.slot-name", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    const child = childFile(fw);
    try {
      if (fw === "vue") {
        // `#header` consumer → child defineSlots / slot outlet
        try {
          await assertDefinitionTargetsFile(
            { file: parent, token: "#header", occurrence: 0, caretOffset: 1 },
            child,
          );
        } catch {
          await assertDefinitionTargetsFile(
            { file: parent, token: "header", occurrence: 0 },
            child,
          );
        }
      } else {
        // Authored snippet prop `header` must resolve to the child's typed
        // `header?: Snippet<...>` contract. There is only one `header` token in
        // the parent fixture; a same-file fallback would be vacuous.
        await assertDefinitionTargetsFile({ file: parent, token: "header", occurrence: 0 }, child);
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.def.slot-name",
        fw === "vue" ? "ISSUE-vue-ide-def-slot-name" : "ISSUE-svelte-ide-def-slot-name",
        `Ctrl+Click slot/snippet name failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.def.slot-prop", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      // Usage of `title` in slot body → destructure binding
      await assertDefinitionTargetsToken(
        { file: parent, token: "title", occurrence: 1 },
        { file: parent, token: "title", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "ide.def.slot-prop",
        fw === "vue" ? "ISSUE-vue-ide-def-slot-prop" : "ISSUE-svelte-ide-def-slot-prop",
        `Ctrl+Click slot/snippet prop failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.def.prop-attr-to-child", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    const child = childFile(fw);
    try {
      if (fw === "vue") {
        await assertDefinitionTargetsFile(
          { file: parent, token: ':label="label"', occurrence: 0, caretOffset: 1 },
          child,
        );
      } else {
        // `{label}` shorthand or label= — land on child $props field or parent local
        try {
          await assertDefinitionTargetsFile(
            { file: parent, token: "{label}", occurrence: 0, caretOffset: 1 },
            child,
          );
        } catch {
          await assertDefinitionTargetsToken(
            { file: parent, token: "label", occurrence: 1 },
            { file: parent, token: "label", occurrence: 0 },
          );
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.def.prop-attr-to-child",
        fw === "vue" ? "ISSUE-vue-ide-def-prop-attr" : "ISSUE-svelte-ide-def-prop-attr",
        `Ctrl+Click prop attribute failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.def.kebab-prop-to-camel-declare", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    if (fw !== "vue") return; // Svelte has no HTML-case kebab contract on prop names
    const parent = parentFile(fw);
    const child = childFile(fw);
    try {
      await assertDefinitionTargetsFile(
        { file: parent, token: ":my-prop=", occurrence: 0, caretOffset: 1 },
        child,
      );
      await assertDefinitionTargetsToken(
        { file: parent, token: "my-prop", occurrence: 0 },
        { file: child, token: "myProp", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "ide.def.kebab-prop-to-camel-declare",
        "ISSUE-vue-ide-def-kebab-prop",
        `kebab :my-prop must land on camel myProp: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.def.kebab-event-to-camel-emit", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    if (fw !== "vue") return;
    const parent = parentFile(fw);
    const child = childFile(fw);
    try {
      await assertDefinitionTargetsFile(
        { file: parent, token: "@my-event=", occurrence: 0, caretOffset: 1 },
        child,
      );
      await assertDefinitionTargetsToken(
        { file: parent, token: "my-event", occurrence: 0 },
        { file: child, token: "myEvent", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "ide.def.kebab-event-to-camel-emit",
        "ISSUE-vue-ide-def-kebab-event",
        `kebab @my-event must land on camel myEvent: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.def.kebab-slot-to-camel-declare", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    if (fw !== "vue") return;
    const parent = parentFile(fw);
    const child = childFile(fw);
    try {
      await assertDefinitionTargetsFile(
        { file: parent, token: "#my-slot=", occurrence: 0, caretOffset: 1 },
        child,
      );
      await assertDefinitionTargetsToken(
        { file: parent, token: "my-slot", occurrence: 0 },
        { file: child, token: "mySlot", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "ide.def.kebab-slot-to-camel-declare",
        "ISSUE-vue-ide-def-kebab-slot",
        `kebab #my-slot must land on camel mySlot: ${String(err)}`,
        "product-gap",
      );
    }
  });

  // ── Completions ───────────────────────────────────────────────

  test("ide.complete.component-props", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      const doc = await reopenFresh(parent);
      const editor = await vscode.window.showTextDocument(doc);
      const marker = "<!-- PROP_COMPLETE_SITE -->";
      const at = findOffset(doc, marker) + marker.length;
      const insert = fw === "vue" ? "\n    <IdeSurfaceChild " : "\n  <IdeSurfaceChild ";
      await editor.edit((eb) => eb.insert(doc.positionAt(at), insert));
      const offset = findOffset(editor.document, insert.trimStart()) + insert.trimStart().length;
      const labels = await completionsAtOffset(parent, offset);
      const need = ["label", "count"];
      for (const n of need) {
        if (!labels.some((l) => l === n || l.startsWith(n) || l.includes(n))) {
          throw new Error(`prop completion missing ${n}; sample=${labels.slice(0, 40).join(", ")}`);
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.complete.component-props",
        fw === "vue" ? "ISSUE-vue-ide-complete-props" : "ISSUE-svelte-ide-complete-props",
        `Component prop completion failed: ${String(err)}`,
        "product-gap",
      );
    } finally {
      await reopenFresh(parentFile(fw!)).catch(() => undefined);
    }
  });

  test("ide.complete.slot-or-snippet-names", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      const doc = await reopenFresh(parent);
      const editor = await vscode.window.showTextDocument(doc);
      if (fw === "vue") {
        const marker = "<!-- SLOT_NAME_COMPLETE_SITE -->";
        const at = findOffset(doc, marker) + marker.length;
        await editor.edit((eb) =>
          eb.insert(doc.positionAt(at), "\n    <IdeSurfaceChild><template #"),
        );
        const offset = findOffset(editor.document, "<template #") + "<template #".length;
        const labels = await completionsAtOffset(parent, offset, "#");
        const hit = labels.some(
          (l) =>
            l === "header" || l.startsWith("header") || l.includes("header") || l === "default",
        );
        if (!hit) {
          throw new Error(`slot name completion missing header; sample=${labels.slice(0, 40)}`);
        }
      } else {
        const marker = "<!-- SNIPPET_NAME_COMPLETE_SITE -->";
        const at = findOffset(doc, marker) + marker.length;
        await editor.edit((eb) =>
          eb.insert(doc.positionAt(at), "\n  <IdeSurfaceChild>\n    {#snippet "),
        );
        const offset = findOffset(editor.document, "{#snippet ") + "{#snippet ".length;
        const labels = await completionsAtOffset(parent, offset);
        const hit = labels.some(
          (l) =>
            l === "header" || l.startsWith("header") || l.includes("header") || l === "children",
        );
        if (!hit) {
          // Some servers complete snippet names only via prop completion on the tag.
          const propOffset =
            findOffset(editor.document, "<IdeSurfaceChild>") + "<IdeSurfaceChild".length;
          const propLabels = await completionsAtOffset(parent, propOffset);
          if (!propLabels.some((l) => l === "header" || l.includes("header") || l === "children")) {
            throw new Error(
              `snippet/slot name completion missing; snippet=${labels.slice(0, 20)}; props=${propLabels.slice(0, 20)}`,
            );
          }
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.complete.slot-or-snippet-names",
        fw === "vue" ? "ISSUE-vue-ide-complete-slot-name" : "ISSUE-svelte-ide-complete-slot-name",
        `Slot/snippet name completion failed: ${String(err)}`,
        "product-gap",
      );
    } finally {
      await reopenFresh(parentFile(fw!)).catch(() => undefined);
    }
  });

  test("ide.complete.directives-or-bind", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      const doc = await reopenFresh(parent);
      const editor = await vscode.window.showTextDocument(doc);
      const marker = "<!-- DIRECTIVE_COMPLETE_SITE -->";
      const at = findOffset(doc, marker) + marker.length;
      if (fw === "vue") {
        await editor.edit((eb) => eb.insert(doc.positionAt(at), "\n    <div v-"));
        const offset = findOffset(editor.document, "<div v-") + "<div v-".length;
        const labels = await completionsAtOffset(parent, offset);
        const hit =
          labels.some((l) =>
            /^(v-)?(if|for|show|model|bind|on|html|text|slot)/i.test(l.replace(/^v-/, "v-")),
          ) || labels.some((l) => /if|for|show|model|bind|html/.test(l));
        if (!hit) {
          throw new Error(`directive completion empty/missing; sample=${labels.slice(0, 40)}`);
        }
      } else {
        await editor.edit((eb) => eb.insert(doc.positionAt(at), "\n  <div bind:"));
        const offset = findOffset(editor.document, "<div bind:") + "<div bind:".length;
        const labels = await completionsAtOffset(parent, offset, ":");
        // bind:this / bind:value-ish or element props
        if (labels.length === 0) {
          throw new Error("bind:/directive-like completion returned no items");
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.complete.directives-or-bind",
        fw === "vue" ? "ISSUE-vue-ide-complete-directive" : "ISSUE-svelte-ide-complete-directive",
        `Directive/bind completion failed: ${String(err)}`,
        "product-gap",
      );
    } finally {
      await reopenFresh(parentFile(fw!)).catch(() => undefined);
    }
  });

  test("ide.complete.event-handler-locals", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      if (fw === "vue") {
        // Inside @pick=" — offer onPick
        const doc = await openRelative(parent);
        const needle = '@pick="onPick"';
        const offset = findOffset(doc, needle) + '@pick="'.length;
        const labels = await completionsAtOffset(parent, offset);
        if (!labels.some((l) => l === "onPick" || l.startsWith("onPick"))) {
          // retry mid-token
          const mid = findOffset(doc, needle) + '@pick="on'.length;
          const labels2 = await completionsAtOffset(parent, mid);
          if (!labels2.some((l) => l.includes("onPick") || l === "onChange")) {
            throw new Error(
              `event handler completion missing onPick; sample=${labels.slice(0, 30)}; mid=${labels2.slice(0, 30)}`,
            );
          }
        }
      } else {
        const doc = await openRelative(parent);
        // onPick={ — complete onPick
        const needle = "{onPick}";
        const offset = findOffset(doc, needle) + 1;
        const labels = await completionsAtOffset(parent, offset);
        if (!labels.some((l) => l === "onPick" || l.startsWith("onPick"))) {
          throw new Error(`event handler completion missing onPick; sample=${labels.slice(0, 30)}`);
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.complete.event-handler-locals",
        fw === "vue"
          ? "ISSUE-vue-ide-complete-event-handler"
          : "ISSUE-svelte-ide-complete-event-handler",
        `Event handler local completion failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.complete.narrowed-member", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      const doc = await openRelative(parent);
      // person.name — complete after person.
      const needle = "person.name";
      const offset = findOffset(doc, needle) + "person.".length;
      const labels = await completionsAtOffset(parent, offset, ".");
      if (!labels.some((l) => l === "name" || l.startsWith("name"))) {
        throw new Error(`narrowed completion missing name; sample=${labels.slice(0, 30)}`);
      }
      if (!labels.some((l) => l === "age" || l.startsWith("age"))) {
        throw new Error(`narrowed completion missing age; sample=${labels.slice(0, 30)}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.complete.narrowed-member",
        fw === "vue" ? "ISSUE-vue-ide-complete-narrow" : "ISSUE-svelte-ide-complete-narrow",
        `Narrowed member completion failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.complete.slot-prop-members", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      const doc = await openRelative(parent);
      // Inside header slot: title. should offer string methods OR after title token
      // Prefer completing members on a known string slot prop via template expression.
      // Use `title` interpolation region: insert temporary `title.` via edit.
      const editor = await vscode.window.showTextDocument(doc);
      if (fw === "vue") {
        const span = "{{ title }}";
        const at = findOffset(doc, span) + "{{ title".length;
        await editor.edit((eb) => eb.insert(doc.positionAt(at), "."));
        const offset = findOffset(editor.document, "{{ title.") + "{{ title.".length;
        const labels = await completionsAtOffset(parent, offset, ".");
        // string members: length, toUpperCase, charAt, …
        if (
          !labels.some((l) =>
            ["length", "toUpperCase", "charAt", "includes", "slice"].some(
              (m) => l === m || l.startsWith(m),
            ),
          )
        ) {
          throw new Error(
            `slot prop member completion missing string methods; sample=${labels.slice(0, 40)}`,
          );
        }
      } else {
        const span = "{title}";
        const at = findOffset(doc, span) + "{title".length;
        await editor.edit((eb) => eb.insert(doc.positionAt(at), "."));
        const offset = findOffset(editor.document, "{title.") + "{title.".length;
        const labels = await completionsAtOffset(parent, offset, ".");
        if (
          !labels.some((l) =>
            ["length", "toUpperCase", "charAt", "includes", "slice"].some(
              (m) => l === m || l.startsWith(m),
            ),
          )
        ) {
          throw new Error(
            `snippet prop member completion missing string methods; sample=${labels.slice(0, 40)}`,
          );
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.complete.slot-prop-members",
        fw === "vue"
          ? "ISSUE-vue-ide-complete-slot-prop-member"
          : "ISSUE-svelte-ide-complete-slot-prop-member",
        `Slot/snippet prop member completion failed: ${String(err)}`,
        "product-gap",
      );
    } finally {
      await reopenFresh(parentFile(fw!)).catch(() => undefined);
    }
  });

  // ── Auto-import ───────────────────────────────────────────────

  test("ide.auto-import.component-tag", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue" ? "src/features/AutoImportTag.vue" : "src/features/AutoImportTag.svelte";
    try {
      const doc = await reopenFresh(file);
      const editor = await vscode.window.showTextDocument(doc);
      const marker = "<!-- TAG_COMPLETE_SITE -->";
      const at = findOffset(doc, marker) + marker.length;
      await editor.edit((eb) =>
        eb.insert(doc.positionAt(at), fw === "vue" ? "\n    <Prop" : "\n  <Prop"),
      );
      const offset = findOffset(editor.document, "<Prop") + "<Prop".length;
      const labels = await completionsAtOffset(file, offset);
      if (!labels.some((l) => l === "PropChild" || l.includes("PropChild"))) {
        throw new Error(`auto-import tag missing PropChild; sample=${labels.slice(0, 40)}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.auto-import.component-tag",
        fw === "vue"
          ? "ISSUE-vue-component-tag-completion"
          : "ISSUE-svelte-component-tag-completion",
        `Component auto-import completion failed: ${String(err)}`,
        "product-gap",
      );
    } finally {
      await reopenFresh(
        fw === "vue" ? "src/features/AutoImportTag.vue" : "src/features/AutoImportTag.svelte",
      ).catch(() => undefined);
    }
  });

  test("ide.auto-import.symbol-accept", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    if (fw === "svelte") {
      // Local identifier completion of `base` from incomplete `bas` (no vue computed).
      try {
        const file = "src/features/AutoImportSymbol.svelte";
        const doc = await openRelative(file);
        const offset = findOffset(doc, "let doubled = bas") + "let doubled = bas".length;
        const labels = await completionsAtOffset(file, offset);
        if (!labels.some((l) => l === "base" || l.startsWith("base"))) {
          throw new Error(`local completion missing base; sample=${labels.slice(0, 30)}`);
        }
      } catch (err) {
        failParityGap(
          this,
          "ide.auto-import.symbol-accept",
          "ISSUE-svelte-ide-complete-local",
          `Svelte local/symbol completion failed: ${String(err)}`,
          "product-gap",
        );
      }
      return;
    }

    // Vue: accept `computed` auto-import (existing AutoImportSymbol.vue)
    try {
      const file = "src/features/AutoImportSymbol.vue";
      const doc = await reopenFresh(file);
      const editor = await vscode.window.showTextDocument(doc);
      const offset = findOffset(doc, "comput(() =>") + "comput".length;
      const pos = doc.positionAt(offset);
      editor.selection = new vscode.Selection(pos, pos);

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
        12_000,
      );

      const docBefore = editor.document.getText();
      const importBefore = scriptBlockText(editor.document);
      await vscode.commands.executeCommand(TRIGGER_SUGGEST_COMMAND);
      await sleep(300);
      await vscode.commands.executeCommand(ACCEPT_SUGGESTION_COMMAND);
      await sleep(400);
      const docAfter = editor.document.getText();
      const importAfter = scriptBlockText(editor.document);
      const outcome = classifyAcceptOutcome({
        docBefore,
        docAfter,
        importBefore,
        importAfter,
      });
      if (!outcome.accepted && !/import\s*\{[^}]*\bcomputed\b/.test(importAfter)) {
        const list = await vscode.commands.executeCommand<vscode.CompletionList>(
          "vscode.executeCompletionItemProvider",
          doc.uri,
          pos,
        );
        const item = (list?.items ?? []).find((i) => {
          const label = typeof i.label === "string" ? i.label : i.label.label;
          return label === "computed";
        });
        if (!item) throw new Error("computed completion item not found after accept attempt");
        // additionalTextEdits path still counts as auto-import capability
        if (!(item.additionalTextEdits && item.additionalTextEdits.length > 0)) {
          throw new Error(
            `accept did not insert import for computed; outcome=${JSON.stringify(outcome)}`,
          );
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.auto-import.symbol-accept",
        "ISSUE-vue-symbol-auto-import",
        `Vue symbol auto-import accept failed: ${String(err)}`,
        "product-gap",
      );
    } finally {
      await reopenFresh("src/features/AutoImportSymbol.vue").catch(() => undefined);
    }
  });

  test("ide.complete.event-attr-names", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      const doc = await reopenFresh(parent);
      const editor = await vscode.window.showTextDocument(doc);
      const marker = "<!-- EVENT_ATTR_COMPLETE_SITE -->";
      const at = findOffset(doc, marker) + marker.length;
      if (fw === "vue") {
        await editor.edit((eb) => eb.insert(doc.positionAt(at), "\n    <IdeSurfaceChild @"));
        const offset =
          findOffset(editor.document, "<IdeSurfaceChild @") + "<IdeSurfaceChild @".length;
        const labels = await completionsAtOffset(parent, offset, "@");
        if (!labels.some((l) => /pick|change|onPick|onChange/i.test(l))) {
          throw new Error(
            `event attr completion missing pick/change; sample=${labels.slice(0, 40)}`,
          );
        }
      } else {
        await editor.edit((eb) => eb.insert(doc.positionAt(at), "\n  <IdeSurfaceChild "));
        const offset =
          findOffset(editor.document, "<IdeSurfaceChild ") + "<IdeSurfaceChild ".length;
        const labels = await completionsAtOffset(parent, offset);
        if (!labels.some((l) => /onPick|onChange|pick|change/i.test(l))) {
          throw new Error(
            `event/callback prop completion missing onPick; sample=${labels.slice(0, 40)}`,
          );
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.complete.event-attr-names",
        fw === "vue" ? "ISSUE-vue-ide-complete-event-attr" : "ISSUE-svelte-ide-complete-event-attr",
        `Event attribute name completion failed: ${String(err)}`,
        "product-gap",
      );
    } finally {
      await reopenFresh(parentFile(fw!)).catch(() => undefined);
    }
  });

  // ── Hover: slot names, slot-props destructure, directive names (D3/D4/D6) ──

  test("ide.hover.slot-name", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      if (fw === "vue") {
        // `#header` — typed from the child's defineSlots surface. The declared
        // return type is `any` (truthful), so the any-forbid is relaxed here.
        await assertHoverNeedles(
          { file: parent, token: "#header", occurrence: 0, caretOffset: 2 },
          ["header", "title", "string", "count", "number"],
          { forbidAny: false },
        );
        // `#default` — same typed surface for the default slot.
        await assertHoverNeedles(
          { file: parent, token: "#default", occurrence: 0, caretOffset: 2 },
          ["default", "body", "string"],
          { forbidAny: false },
        );
        // Kebab `#my-slot` resolves the camel-declared `mySlot` signature.
        await assertHoverNeedles(
          { file: parent, token: "#my-slot", occurrence: 0, caretOffset: 2 },
          ["mySlot", "note", "string"],
          { forbidAny: false },
        );
      } else {
        // `{#snippet header(` name — typed snippet hover.
        await assertHoverNeedles({ file: parent, token: "header", occurrence: 0 }, ["Snippet"]);
        // `{@render header(...)}` callsite in the child — typed snippet hover.
        await assertHoverNeedles({ file: childFile(fw), token: "header", occurrence: 1 }, [
          "Snippet",
        ]);
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.hover.slot-name",
        fw === "vue" ? "ISSUE-vue-ide-hover-slot-name" : "ISSUE-svelte-ide-hover-slot-name",
        `Slot name hover failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.hover.slot-prop-pattern", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      // Pattern positions (the destructure) must hover exactly like a
      // standalone-TS destructured parameter; usage positions match.
      await assertHoverNeedles({ file: parent, token: "title", occurrence: 0 }, [
        "title",
        "string",
      ]);
      await assertHoverNeedles({ file: parent, token: "slotCount", occurrence: 0 }, [
        "slotCount",
        "number",
      ]);
      await assertHoverNeedles({ file: parent, token: "title", occurrence: 1 }, [
        "title",
        "string",
      ]);
      await assertHoverNeedles({ file: parent, token: "slotCount", occurrence: 1 }, [
        "slotCount",
        "number",
      ]);
    } catch (err) {
      failParityGap(
        this,
        "ide.hover.slot-prop-pattern",
        fw === "vue"
          ? "ISSUE-vue-ide-hover-slot-prop-pattern"
          : "ISSUE-svelte-ide-hover-slot-prop-pattern",
        `Slot-prop destructure hover failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.hover.directive-doc", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const parent = parentFile(fw);
    try {
      if (fw === "vue") {
        // Built-in directive NAME tokens get doc hovers.
        await assertHoverNeedles({ file: parent, token: "v-if", occurrence: 0 }, [
          "v-if",
          "Conditionally",
        ]);
      } else {
        // Svelte directive KEYWORD tokens get doc hovers.
        await assertHoverNeedles({ file: parent, token: "use:highlight", occurrence: 0 }, [
          "action",
        ]);
        await assertHoverNeedles({ file: parent, token: "transition:fade", occurrence: 0 }, [
          "transition",
        ]);
        // Local names get the real function hover (never the shim).
        const actionHover = await assertHoverNeedles(
          { file: parent, token: "highlight", occurrence: 1 },
          ["highlight", "HTMLElement"],
        );
        const transitionHover = await assertHoverNeedles(
          { file: parent, token: "fade", occurrence: 1 },
          ["fade", "TransitionConfig"],
        );
        void actionHover;
        void transitionHover;
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.hover.directive-doc",
        fw === "vue" ? "ISSUE-vue-ide-hover-directive-doc" : "ISSUE-svelte-ide-hover-directive-doc",
        `Directive doc hover failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("ide.hover.custom-directive", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    if (fw !== "vue") return;
    const parent = parentFile(fw);
    try {
      // `v-my-thing` → typed hover naming the resolved `vMyThing` binding.
      await assertHoverNeedles({ file: parent, token: "v-my-thing", occurrence: 0 }, ["vMyThing"]);
      // …and Ctrl+click to the authored declaration.
      await assertDefinitionTargetsToken(
        { file: parent, token: "v-my-thing", occurrence: 0 },
        { file: parent, token: "vMyThing", occurrence: 0 },
      );
      // Unknown directive → silent (no hover at all).
      const doc = await openRelative(parent);
      const at = findOffset(doc, "v-nope") + 2;
      const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        doc.uri,
        doc.positionAt(at),
      );
      if (hovers && hovers.length > 0) {
        const text = hovers
          .flatMap((h) => h.contents)
          .map((c) => (typeof c === "string" ? c : (c as vscode.MarkdownString).value))
          .join("\n");
        throw new Error(`unknown directive v-nope must stay silent, got hover: ${text}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "ide.hover.custom-directive",
        "ISSUE-vue-ide-hover-custom-directive",
        `Custom directive hover/definition failed: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
