/**
 * Find-all-references + rename exactness for TS and JS on both frameworks.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertReferenceCountAtLeast,
  assertRenameCoversAndRestores,
  definitionsAt,
  ensureParityReady,
  referencesAt,
  failParityGap,
  VIRTUAL_CARRIER,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`Find and rename exactness [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("shared.find.ts.exact-min-set", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const refs = await referencesAt({ file, token: "dailyValue", occurrence: 0 });
      if (refs.length < 4) throw new Error(`expected >=4 refs, got ${refs.length}`);
      for (const r of refs) {
        if (VIRTUAL_CARRIER.test(r.uri.fsPath)) throw new Error(`virtual ref ${r.uri.fsPath}`);
        if (!r.uri.fsPath.replace(/\\/g, "/").endsWith(file.replace(/^\//, ""))) {
          // same-file binding set for this fixture
          throw new Error(`cross-file ref unexpected: ${r.uri.fsPath}`);
        }
      }
    } catch (err) {
      failParityGap(this, "shared.find.ts.exact-min-set", "ISSUE-find-ts-exact", String(err));
    }
  });

  test("shared.find.js.exact-min-set", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/matrix/JsSurface.vue" : "src/matrix/JsSurface.svelte";
    try {
      await assertReferenceCountAtLeast({ file, token: "jsCount", occurrence: 0 }, 3);
    } catch (err) {
      failParityGap(this, "shared.find.js.exact-min-set", "ISSUE-find-js-exact", String(err));
    }
  });

  test("shared.find.function.cross-region", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      await assertReferenceCountAtLeast({ file, token: "renderDaily", occurrence: 0 }, 2);
    } catch (err) {
      failParityGap(this, "shared.find.function.cross-region", "ISSUE-find-function", String(err));
    }
  });

  test("shared.rename.ts.markup-origin", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      await assertRenameCoversAndRestores(
        { file, token: "dailyValue", occurrence: 3 },
        "dailyDatum",
        { minEdits: 3 },
      );
    } catch (err) {
      failParityGap(this, "shared.rename.ts.markup-origin", "ISSUE-rename-ts-markup", String(err));
    }
  });

  test("shared.rename.js.function", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/matrix/JsSurface.vue" : "src/matrix/JsSurface.svelte";
    try {
      await assertRenameCoversAndRestores({ file, token: "jsBump", occurrence: 0 }, "jsBumpX", {
        minEdits: 2,
      });
    } catch (err) {
      failParityGap(this, "shared.rename.js.function", "ISSUE-rename-js-function", String(err));
    }
  });

  test("shared.find.definition-then-refs-consistency", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/features/MappingCase.vue" : "src/features/MappingCase.svelte";
    try {
      const defs = await definitionsAt({ file, token: "mapValue", occurrence: 2 });
      if (defs.length === 0) throw new Error("no definition");
      const refs = await referencesAt({ file, token: "mapValue", occurrence: 0 });
      if (refs.length < 2) throw new Error(`refs ${refs.length}`);
      // definition target file should appear in reference set
      const defPath = defs[0].uri.fsPath;
      if (!refs.some((r) => r.uri.fsPath === defPath)) {
        throw new Error("definition file not in reference set");
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.find.definition-then-refs-consistency",
        "ISSUE-find-def-refs-consistency",
        String(err),
      );
    }
  });
});
