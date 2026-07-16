/**
 * Rename, document/workspace symbols — both parity fixtures.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertRenameCoversAndRestores,
  documentSymbolsAt,
  ensureParityReady,
  prepareRenameAt,
  failParityGap,
  workspaceSymbolsMatching,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`Rename and symbols [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("shared.rename.prepare-script-binding", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const prepared = await prepareRenameAt({ file, token: "dailyValue", occurrence: 0 });
      if (!prepared) throw new Error("prepareRename returned empty");
    } catch (err) {
      failParityGap(
        this,
        "shared.rename.prepare-script-binding",
        "ISSUE-shared-rename-prepare",
        `prepareRename failed for ${fw}: ${String(err)}`,
      );
    }
  });

  test("shared.rename.from-script.applies", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      await assertRenameCoversAndRestores(
        { file, token: "dailyValue", occurrence: 0 },
        "dailyDatum",
        {
          minEdits: 2,
          definitionFrom: { file, token: "dailyValue", occurrence: 3 },
          definitionTo: { file, token: "dailyValue", occurrence: 0 },
        },
      );
    } catch (err) {
      failParityGap(
        this,
        "shared.rename.from-script.applies",
        "ISSUE-shared-rename-apply",
        `Rename apply/restore failed for ${fw}: ${String(err)}`,
      );
    }
  });

  test("shared.rename.from-markup.applies", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      await assertRenameCoversAndRestores(
        { file, token: "dailyValue", occurrence: 3 },
        "dailyDatum",
        { minEdits: 2 },
      );
    } catch (err) {
      failParityGap(
        this,
        "shared.rename.from-markup.applies",
        "ISSUE-shared-rename-from-markup",
        `Rename from markup failed for ${fw}: ${String(err)}`,
      );
    }
  });

  test("shared.document-symbols.present", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const symbols = await documentSymbolsAt(file);
      if (symbols.length === 0) throw new Error("no document symbols");
      const names = symbols.map((s) => ("name" in s ? s.name : String(s)));
      if (!names.some((n) => /dailyValue|DailyBinding|renderDaily/i.test(n))) {
        throw new Error(`unexpected symbols: ${names.join(", ")}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.document-symbols.present",
        "ISSUE-shared-document-symbols",
        `Document symbols incomplete for ${fw}: ${String(err)}`,
      );
    }
  });

  test("shared.workspace-symbols.find-binding", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    try {
      const symbols = await workspaceSymbolsMatching("dailyValue");
      if (symbols.length === 0) throw new Error("workspace symbol search empty");
      const leaked = symbols.filter((s) =>
        /\.(vue|svelte)\.(tsx|jsx|verter\.ts)/i.test(s.location.uri.fsPath),
      );
      if (leaked.length > 0) {
        throw new Error(`workspace symbols leaked virtual paths: ${leaked[0].location.uri.fsPath}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.workspace-symbols.find-binding",
        "ISSUE-shared-workspace-symbols",
        `Workspace symbols incomplete for ${fw}: ${String(err)}`,
      );
    }
  });
});
