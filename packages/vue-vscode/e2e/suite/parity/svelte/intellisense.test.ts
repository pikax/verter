/**
 * Svelte intellisense: component tag completion, {#if} narrowing.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertHoverLooksNarrowed,
  assertHoverNeedles,
  completionsAtOffset,
  ensureParityReady,
  findOffset,
  hoverTextAt,
  openRelative,
  failParityGap,
} from "../../../lib/parityHarness";

function onlySvelteParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "svelte-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Svelte intellisense [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlySvelteParity(this);
    await ensureParityReady("src/App.svelte");
  });

  test("svelte.intellisense.component-tag.completion", async function () {
    onlySvelteParity(this);
    this.timeout(30_000);
    try {
      const doc = await openRelative("src/features/AutoImportTag.svelte");
      const vscode = await import("vscode");
      const editor = await vscode.window.showTextDocument(doc);
      const marker = "<!-- TAG_COMPLETE_SITE -->";
      const at = findOffset(doc, marker) + marker.length;
      await editor.edit((eb) => eb.insert(doc.positionAt(at), "\n  <Prop"));
      const offset = findOffset(editor.document, "<Prop") + "<Prop".length;
      const labels = await completionsAtOffset("src/features/AutoImportTag.svelte", offset);
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
        "svelte.intellisense.component-tag.completion",
        "ISSUE-svelte-component-tag-completion",
        `Svelte component tag completion failed: ${String(err)}`,
      );
    }
  });

  test("svelte.intellisense.if.narrowing.hover", async function () {
    onlySvelteParity(this);
    try {
      const text = await hoverTextAt({
        file: "src/features/Narrowing.svelte",
        token: "person",
        occurrence: 2,
      });
      assertHoverLooksNarrowed(text, ["Person", "name"]);
    } catch (err) {
      try {
        await assertHoverNeedles(
          { file: "src/features/Narrowing.svelte", token: "name", occurrence: 1 },
          ["string", "name"],
        );
      } catch (inner) {
        failParityGap(
          this,
          "svelte.intellisense.if.narrowing.hover",
          "ISSUE-svelte-if-narrowing-intellisense",
          `{#if} narrowing hover failed: ${String(err)}; retry: ${String(inner)}`,
        );
      }
    }
  });

  test("svelte.intellisense.if.narrowing.completion", async function () {
    onlySvelteParity(this);
    try {
      const doc = await openRelative("src/features/Narrowing.svelte");
      const offset = findOffset(doc, "person.name") + "person.".length;
      const labels = await completionsAtOffset("src/features/Narrowing.svelte", offset, ".");
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
        "svelte.intellisense.if.narrowing.completion",
        "ISSUE-svelte-if-narrowing-completion",
        `{#if} member completion failed: ${String(err)}`,
      );
    }
  });
});
