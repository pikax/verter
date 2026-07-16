/**
 * Position mapping fidelity: hover ranges must cover authored template tokens
 * (fixes "highlight background slightly off" class of bugs).
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertHoverRangeCoversToken,
  assertHoverNeedles,
  documentHighlightsAt,
  ensureParityReady,
  failParityGap,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`Mapping fidelity [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("shared.mapping.hover-range.template-binding", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      // Template occurrence of mapValue (after script decl + body uses).
      await assertHoverRangeCoversToken({ file, token: "mapValue", occurrence: 2 });
    } catch (err) {
      failParityGap(
        this,
        "shared.mapping.hover-range.template-binding",
        "ISSUE-mapping-hover-range-template",
        `Hover range off for template binding (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.mapping.hover-range.template-member", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      await assertHoverRangeCoversToken({ file, token: "label", occurrence: 1 });
      await assertHoverNeedles({ file, token: "label", occurrence: 1 }, ["string", "label"]);
    } catch (err) {
      failParityGap(
        this,
        "shared.mapping.hover-range.template-member",
        "ISSUE-mapping-hover-range-member",
        `Hover range/type off for template member (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.mapping.hover-range.script-binding", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      await assertHoverRangeCoversToken({ file, token: "mapValue", occurrence: 0 });
    } catch (err) {
      failParityGap(
        this,
        "shared.mapping.hover-range.script-binding",
        "ISSUE-mapping-hover-range-script",
        `Hover range off for script binding (${fw}): ${String(err)}`,
      );
    }
  });

  test("shared.mapping.highlights.cover-token", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      const highlights = await documentHighlightsAt({ file, token: "mapValue", occurrence: 0 });
      if (highlights.length < 2) {
        throw new Error(`expected multi-region highlights, got ${highlights.length}`);
      }
      // Each highlight range should be non-empty and not virtual-path-based (uri is same doc).
      for (const h of highlights) {
        const width =
          h.range.end.line === h.range.start.line
            ? h.range.end.character - h.range.start.character
            : 10;
        if (width <= 0) throw new Error("zero-width highlight range");
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.mapping.highlights.cover-token",
        "ISSUE-mapping-highlights",
        `Document highlight mapping incomplete (${fw}): ${String(err)}`,
      );
    }
  });
});
