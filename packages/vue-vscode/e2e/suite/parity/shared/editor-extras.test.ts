/**
 * Editor features: call hierarchy, folding, selection range, document links.
 */
import * as path from "node:path";
import { FIXTURE_NAME } from "../../../helpers";
import {
  absoluteFile,
  documentLinksFor,
  ensureParityReady,
  foldingRangesFor,
  prepareCallHierarchyAt,
  incomingCalls,
  outgoingCalls,
  selectionRangesAt,
  failParityGap,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`Editor extras [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("shared.editor.folding-ranges.blocks", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      const ranges = await foldingRangesFor(file);
      if (ranges.length < 1) throw new Error("no folding ranges");
      // Vue SFC should fold at least script and/or template-ish regions.
      const multiLine = ranges.filter((r) => r.end > r.start);
      if (multiLine.length < 1) throw new Error(`no multi-line folds: ${ranges.length}`);
    } catch (err) {
      failParityGap(
        this,
        "shared.editor.folding-ranges.blocks",
        "ISSUE-editor-folding",
        `Folding ranges incomplete (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("shared.editor.selection-range.expression", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      const ranges = await selectionRangesAt({ file, token: "mapValue", occurrence: 2 });
      if (!ranges || ranges.length === 0) throw new Error("no selection ranges");
      const first = ranges[0];
      if (!first.range) throw new Error("selection range missing range");
    } catch (err) {
      failParityGap(
        this,
        "shared.editor.selection-range.expression",
        "ISSUE-editor-selection-range",
        `Selection range incomplete (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("shared.editor.document-links.import", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    // PropParent imports PropChild — links should resolve to authored child file.
    const file =
      fw === "vue" ? "src/components/PropParent.vue" : "src/components/PropParent.svelte";
    const child = fw === "vue" ? "src/components/PropChild.vue" : "src/components/PropChild.svelte";
    try {
      const links = await documentLinksFor(file);
      if (links.length === 0) throw new Error("no document links");
      const childAbs = absoluteFile(child).toLowerCase();
      const hit = links.some((link) => {
        const target = link.target?.fsPath?.toLowerCase();
        return (
          target === childAbs || (target?.endsWith(path.basename(child).toLowerCase()) ?? false)
        );
      });
      if (!hit) {
        throw new Error(
          `no link to ${child}; got ${links
            .map((l) => l.target?.fsPath ?? "(no target)")
            .slice(0, 8)
            .join(", ")}`,
        );
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.editor.document-links.import",
        "ISSUE-editor-document-links",
        `Document links incomplete (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("shared.editor.call-hierarchy.function", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      const items = await prepareCallHierarchyAt({ file, token: "mapFn", occurrence: 0 });
      if (!items || items.length === 0) throw new Error("prepareCallHierarchy empty");
      const item = items[0];
      const incoming = await incomingCalls(item);
      const outgoing = await outgoingCalls(item);
      // At least one direction responds with an array (may be empty if no callers).
      if (incoming === undefined && outgoing === undefined) {
        throw new Error("neither incoming nor outgoing call hierarchy available");
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.editor.call-hierarchy.function",
        "ISSUE-editor-call-hierarchy",
        `Call hierarchy incomplete (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });
});
