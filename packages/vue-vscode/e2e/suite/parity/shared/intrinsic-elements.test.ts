/**
 * Intrinsic HTML/SVG element interfaces (hover + attr types + type-definition).
 *
 * First-class for **both** Vue and Svelte: same cases, same bar, parallel ISSUE ids.
 * A native tag (e.g. `<div>`) must resolve to a concrete DOM interface — never an
 * open-index surface such as `(index) IntrinsicElements[string]: any`.
 *
 * Keeps a small representative tag set so the suite stays fast under the default
 * Mocha timeout (no multi-minute per-test budgets).
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  ensureParityReady,
  hoverTextAt,
  failParityGap,
  typeDefinitionsAt,
  type TokenAnchor,
} from "../../../lib/parityHarness";
import {
  HTML_INTRINSIC_ATTRS,
  HTML_INTRINSIC_TAGS,
  assertIntrinsicAttrHoverText,
  assertIntrinsicElementHoverText,
  intrinsicElementsFile,
  looksLikeOpenIntrinsicIndex,
} from "../../../lib/intrinsicElementTypes";

function parityFramework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

/**
 * Parallel ISSUE ids — identical shape for Vue and Svelte (no preferred framework).
 * Literals required so `gen-issues-ledger.mjs` / issueLedger can discover them.
 */
function issueFor(
  fw: "vue" | "svelte",
  kind: "tag-hover" | "attr-hover" | "type-definition" | "clean",
): string {
  const table = {
    vue: {
      "tag-hover": "ISSUE-vue-intrinsic-element-hover",
      "attr-hover": "ISSUE-vue-intrinsic-attr-hover",
      "type-definition": "ISSUE-vue-intrinsic-type-definition",
      clean: "ISSUE-vue-intrinsic-elements-clean",
    },
    svelte: {
      "tag-hover": "ISSUE-svelte-intrinsic-element-hover",
      "attr-hover": "ISSUE-svelte-intrinsic-attr-hover",
      "type-definition": "ISSUE-svelte-intrinsic-type-definition",
      clean: "ISSUE-svelte-intrinsic-elements-clean",
    },
  } as const;
  return table[fw][kind];
}

function tagAnchor(file: string, openTagToken: string, caretOffset = 1): TokenAnchor {
  return { file, token: openTagToken, occurrence: 0, caretOffset };
}

/** Small discriminating set: container, form control, link, media/svg. */
const TAGS_UNDER_TEST = HTML_INTRINSIC_TAGS.filter((t) =>
  (["div", "input", "a", "button", "svg"] as const).includes(
    t.tag as "div" | "input" | "a" | "button" | "svg",
  ),
);

const ATTRS_UNDER_TEST = HTML_INTRINSIC_ATTRS.filter((a) =>
  (["a.href", "input.type", "div.class"] as const).includes(
    a.id as "a.href" | "input.type" | "div.class",
  ),
);

suite(`Intrinsic element interfaces [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = parityFramework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    // Warm only — inherits Mocha default when root already synced; cap if cold.
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("intrinsic.clean-diagnostics", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = intrinsicElementsFile(fw);
    try {
      await assertCleanErrors(file);
    } catch (err) {
      failParityGap(
        this,
        "intrinsic.clean-diagnostics",
        issueFor(fw, "clean"),
        `Intrinsic elements fixture is not clean (missing closed JSX/DOM env likely): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("intrinsic.tag-hover.div-is-concrete-interface", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = intrinsicElementsFile(fw);
    const div = HTML_INTRINSIC_TAGS.find((t) => t.tag === "div")!;
    try {
      const text = await hoverTextAt(tagAnchor(file, div.openTagToken, div.tagCaretOffset ?? 1));
      assertIntrinsicElementHoverText(text, "div", div.anyOf);
    } catch (err) {
      failParityGap(
        this,
        "intrinsic.tag-hover.div-is-concrete-interface",
        issueFor(fw, "tag-hover"),
        `<div> must hover as a concrete element interface, not open IntrinsicElements[string]:any — ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("intrinsic.tag-hover.representative-concrete-interfaces", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = intrinsicElementsFile(fw);
    const failures: string[] = [];
    const texts: Array<{ tag: string; text: string }> = [];

    for (const spec of TAGS_UNDER_TEST) {
      try {
        const text = await hoverTextAt(
          tagAnchor(file, spec.openTagToken, spec.tagCaretOffset ?? 1),
        );
        texts.push({ tag: spec.tag, text });
        assertIntrinsicElementHoverText(text, spec.tag, spec.anyOf);
      } catch (err) {
        failures.push(`<${spec.tag}>: ${String(err)}`);
      }
    }

    const openIndexTags = texts
      .filter((t) => looksLikeOpenIntrinsicIndex(t.text))
      .map((t) => t.tag);
    if (openIndexTags.length > 0) {
      failures.push(
        `open IntrinsicElements[string] (or equivalent) on tags: ${openIndexTags.join(", ")}`,
      );
    }

    // Per-tag types: div vs input must not both be the same open-index any.
    const divHover = texts.find((t) => t.tag === "div");
    const inputHover = texts.find((t) => t.tag === "input");
    if (divHover && inputHover && divHover.text.trim() && inputHover.text.trim()) {
      if (
        looksLikeOpenIntrinsicIndex(divHover.text) &&
        looksLikeOpenIntrinsicIndex(inputHover.text)
      ) {
        failures.push("div and input both open-index any — element types are not per-tag");
      }
    }

    if (failures.length > 0) {
      failParityGap(
        this,
        "intrinsic.tag-hover.representative-concrete-interfaces",
        issueFor(fw, "tag-hover"),
        `Intrinsic tag hover failures (${failures.length}):\n${failures.join("\n")}`,
        "product-gap",
      );
    }
  });

  test("intrinsic.attr-hover.typed-attributes", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = intrinsicElementsFile(fw);
    const failures: string[] = [];

    for (const attr of ATTRS_UNDER_TEST) {
      try {
        const text = await hoverTextAt({
          file,
          token: attr.token,
          occurrence: 0,
          caretOffset: attr.caretOffset ?? 0,
        });
        assertIntrinsicAttrHoverText(text, attr.id, attr.anyOf);
      } catch (err) {
        failures.push(`${attr.id}: ${String(err)}`);
      }
    }

    if (failures.length > 0) {
      failParityGap(
        this,
        "intrinsic.attr-hover.typed-attributes",
        issueFor(fw, "attr-hover"),
        `Intrinsic attribute hover failures:\n${failures.join("\n")}`,
        "product-gap",
      );
    }
  });

  test("intrinsic.type-definition.div-not-any", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = intrinsicElementsFile(fw);
    const div = HTML_INTRINSIC_TAGS.find((t) => t.tag === "div")!;
    try {
      const locs = await typeDefinitionsAt(
        tagAnchor(file, div.openTagToken, div.tagCaretOffset ?? 1),
      );
      if (locs.length === 0) {
        throw new Error("type definition returned no locations for <div>");
      }
      const first = locs[0]!;
      const vscode = await import("vscode");
      const defDoc = await vscode.workspace.openTextDocument(first.uri);
      const snippet = defDoc.getText(
        new vscode.Range(
          Math.max(0, first.range.start.line - 2),
          0,
          Math.min(defDoc.lineCount - 1, first.range.end.line + 4),
          200,
        ),
      );
      if (looksLikeOpenIntrinsicIndex(snippet) || /\[\s*string\s*\]\s*:\s*any/.test(snippet)) {
        throw new Error(`type definition landed on open-index any:\n${snippet}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "intrinsic.type-definition.div-not-any",
        issueFor(fw, "type-definition"),
        `Type definition for <div> missing or open-index: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
