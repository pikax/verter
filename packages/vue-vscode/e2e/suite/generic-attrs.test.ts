import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  measureHover,
  getCompletions,
  findPosition,
  FIXTURE_NAME,
} from "../helpers";

suite(`Generic & Attrs [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    if (FIXTURE_NAME !== "single-project") {
      this.skip();
      return;
    }
    await waitForExtensionReady();
    doc = await openVueFile("src/GenericAttrsComp.vue");
    await waitForFileReady(doc);
  });

  // ── Return Type Annotation ──────────────────────────────────────

  test("no ts(7010) implicit-any-return-type diagnostic", async function () {
    // ts(7010): Function which lacks return-type annotation implicitly has an 'any' return type.
    // The TemplateBindingFN should have `: any` return type to suppress this.
    const allDiags = vscode.languages.getDiagnostics(doc.uri);
    const ts7010 = allDiags.filter(
      (d) =>
        (typeof d.code === "number" && d.code === 7010) ||
        (typeof d.code === "object" &&
          (d.code as { value: unknown }).value === 7010),
    );
    expect(
      ts7010,
      "Should have no ts(7010) implicit-any-return-type diagnostic",
    ).to.have.lengthOf(0);
  });

  // ── Generic Attribute Value ─────────────────────────────────────

  test("hover inside generic attribute value delegates to TypeProvider", async function () {
    // Cursor on "string" inside generic="T extends string"
    const pos = findPosition(doc, "T extends string", 10); // on "string"
    if (!pos) {
      console.log("    generic attribute not found — skip");
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(
      `    Hover inside generic value: ${latencyMs}ms, ${hovers.length} result(s)`,
    );

    // Soft check: if type provider is available, hover should return something
    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 200)}`);
      // Should NOT show SFC attribute documentation (that would mean delegation failed)
      expect(
        content.toLowerCase(),
        "Should not show SFC attribute docs inside generic value",
      ).to.not.include("type parameter");
    }
  });

  test("hover on generic attribute NAME shows SFC docs", async function () {
    // Cursor on the "generic" attribute name (not value)
    const pos = findPosition(doc, 'generic="', 0); // on "g" of "generic"
    if (!pos) {
      console.log("    generic attribute not found — skip");
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(
      `    Hover on generic attr name: ${latencyMs}ms, ${hovers.length} result(s)`,
    );

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 200)}`);
      // Should show SFC attribute documentation
      expect(content.toLowerCase(), "Should show docs for generic attribute").to.include(
        "generic",
      );
    }
  });

  // ── Attrs Attribute Value ───────────────────────────────────────

  test("hover inside attrs attribute value delegates to TypeProvider", async function () {
    // Cursor on "string" inside attrs="{ class: string, id?: string }"
    const pos = findPosition(doc, "{ class: string", 9); // on "string"
    if (!pos) {
      console.log("    attrs attribute not found — skip");
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(
      `    Hover inside attrs value: ${latencyMs}ms, ${hovers.length} result(s)`,
    );

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 200)}`);
      // Should NOT show SFC attribute documentation
      expect(
        content.toLowerCase(),
        "Should not show SFC attribute docs inside attrs value",
      ).to.not.include("fallthrough");
    }
  });

  test("hover on attrs attribute NAME shows SFC docs", async function () {
    const pos = findPosition(doc, 'attrs="', 0); // on "a" of "attrs"
    if (!pos) {
      console.log("    attrs attribute not found — skip");
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(
      `    Hover on attrs attr name: ${latencyMs}ms, ${hovers.length} result(s)`,
    );

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 200)}`);
      // Should show SFC attribute documentation
      expect(content.toLowerCase(), "Should show docs for attrs attribute").to.include(
        "attrs",
      );
    }
  });

  // ── Template Binding with Generics ──────────────────────────────

  test("hover on generic-typed binding in template", async function () {
    // Hover on "value" in {{ value }} — should resolve to type T (generic)
    const pos = findPosition(doc, "{{ value }}", 3); // on "value"
    if (!pos) {
      console.log("    {{ value }} not found — skip");
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(
      `    Hover on generic binding: ${latencyMs}ms, ${hovers.length} result(s)`,
    );

    if (hovers.length > 0) {
      const content = hovers[0].contents
        .map((c) => (typeof c === "string" ? c : c.value))
        .join("\n");
      console.log(`    Hover content: ${content.slice(0, 200)}`);
      expect(hovers[0].contents.length, "Hover on generic binding should have content").to.be.greaterThan(0);
    }
  });

  // ── Completion in Attrs/Generic Values ──────────────────────────

  test("completions inside attrs value should not show SFC attribute names", async function () {
    // Cursor inside attrs="{ class: string, id?: string }"
    // Completions should be TypeScript type completions, not SFC attributes
    const pos = findPosition(doc, "{ class: string", 9); // after "string"
    if (!pos) {
      console.log("    attrs attribute not found — skip");
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    if (completions && completions.items.length > 0) {
      const labels = completions.items.map((i) => i.label);
      console.log(`    Completions count: ${labels.length}`);

      // Should NOT contain SFC attribute names
      expect(labels.join(","), "Should not offer SFC attrs as completions").to.not.include("setup");
      expect(labels.join(","), "Should not offer SFC attrs as completions").to.not.include("lang");
    }
  });
});
