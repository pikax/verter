/**
 * Confidence-hardening suite: cases that raise trust beyond “happy path green.”
 *
 * Honest goal: if these fail, we must NOT claim Verter works reliably.
 * If they pass, confidence rises for invalidation, generics/cross-file, multi-file
 * session, and no-virtual leakage — still not “works all the time on every app.”
 *
 * Absolute contracts only (TS + Verter product). No Volar / Svelte Official LS.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME, sleep } from "../../../helpers";
import {
  assertCleanErrors,
  assertDefinitionTargetsToken,
  assertHasErrorMatching,
  assertHoverNeedles,
  definitionsAt,
  ensureParityReady,
  errorDiagnostics,
  openRelative,
  registerFrameworkTest,
  failParityGap,
  verterUnknownPropDiags,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

const TYPE_MISMATCH = /2322|2345|2353|type|assignable|number|string|unknown-prop|Property/i;

async function revertDoc(): Promise<void> {
  try {
    await vscode.commands.executeCommand("workbench.action.files.revert");
  } catch {
    /* best-effort */
  }
}

suite(`Confidence hardening [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("confidence.invalidation.edit-introduces-unknown-prop", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue"
        ? "src/confidence/InvalidateTarget.vue"
        : "src/confidence/InvalidateTarget.svelte";
    try {
      await assertCleanErrors(file);
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      const text = doc.getText();
      // Insert an invented prop into the component tag.
      const needle = fw === "vue" ? '<StrictLeaf label="ok"' : '<StrictChild label="ok"';
      const idx = text.indexOf(needle);
      if (idx < 0) throw new Error(`TEST_DEFECT: missing ${needle}`);
      const insertAt = idx + needle.length;
      await editor.edit((eb) => eb.insert(doc.positionAt(insertAt), ' totallyFakeProp="nope"'));
      await sleep(200);
      // Poll until an error appears (invalidation / recheck).
      const deadline = Date.now() + 10_000;
      let hit = false;
      while (Date.now() < deadline) {
        const errors = await errorDiagnostics(file);
        const verter = verterUnknownPropDiags(editor.document.uri);
        if (
          errors.some((d) => TYPE_MISMATCH.test(`${d.code}:${d.message}`)) ||
          verter.some((d) => /fake|unknown|totally/i.test(d.message))
        ) {
          hit = true;
          break;
        }
        await sleep(200);
      }
      if (!hit) {
        throw new Error("after inventing prop, no type/unknown-prop diagnostic appeared");
      }
      // Restore and require clean again
      await revertDoc();
      await sleep(200);
      await assertCleanErrors(file);
    } catch (err) {
      await revertDoc();
      failParityGap(
        this,
        "confidence.invalidation.edit-introduces-unknown-prop",
        fw === "vue" ? "ISSUE-vue-confidence-invalidation" : "ISSUE-svelte-confidence-invalidation",
        `Invalidation contract failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("confidence.cross-file.wrong-prop-type", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue"
        ? "src/confidence/CrossFileConsumer.vue"
        : "src/confidence/CrossFileConsumer.svelte";
    try {
      await assertHasErrorMatching(file, TYPE_MISMATCH);
    } catch (err) {
      failParityGap(
        this,
        "confidence.cross-file.wrong-prop-type",
        fw === "vue" ? "ISSUE-vue-confidence-cross-file" : "ISSUE-svelte-confidence-cross-file",
        `Cross-file typed wrong prop must error (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  registerFrameworkTest("vue", "confidence.generic-list.wrong-selected-id", async function () {
    try {
      // selected-id is string; 99 is number → must error
      await assertHasErrorMatching("src/confidence/CrossFileConsumer.vue", TYPE_MISMATCH);
    } catch (err) {
      failParityGap(
        this,
        "confidence.generic-list.wrong-selected-id",
        "ISSUE-vue-confidence-generic",
        `GenericList wrong selected-id type must error: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("confidence.multi-file.open.definitions-authored", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const multi = fw === "vue" ? "src/confidence/MultiHop.vue" : "src/confidence/MultiHop.svelte";
    const daily = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      // Open several carriers; definitions must stay on authored paths.
      await openRelative(multi);
      await openRelative(daily);
      await openRelative(
        fw === "vue" ? "src/strict/StrictLeaf.vue" : "src/strict/StrictChild.svelte",
      );
      const locs = await definitionsAt({ file: multi, token: "label", occurrence: 1 });
      if (locs.length === 0) throw new Error("no definition for label in MultiHop");
      const leaked = locs.filter((l) =>
        /\.(vue|svelte)\.(tsx|jsx|verter\.ts|__verter_test\.ts)/i.test(l.uri.fsPath),
      );
      if (leaked.length > 0) {
        throw new Error(`definition leaked virtual path: ${leaked[0]!.uri.fsPath}`);
      }
      await assertDefinitionTargetsToken(
        { file: daily, token: "dailyValue", occurrence: 3 },
        { file: daily, token: "dailyValue", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "confidence.multi-file.open.definitions-authored",
        fw === "vue" ? "ISSUE-vue-confidence-multi-file" : "ISSUE-svelte-confidence-multi-file",
        `Multi-file session definition contract failed (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("confidence.multi-hop.hover-no-any", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/confidence/MultiHop.vue" : "src/confidence/MultiHop.svelte";
    try {
      await assertHoverNeedles({ file, token: "label", occurrence: 0 }, ["label"], {
        forbidAny: true,
      });
      await assertHoverNeedles({ file, token: "onPick", occurrence: 0 }, ["onPick"], {
        forbidAny: true,
      });
    } catch (err) {
      failParityGap(
        this,
        "confidence.multi-hop.hover-no-any",
        fw === "vue" ? "ISSUE-vue-confidence-hover-any" : "ISSUE-svelte-confidence-hover-any",
        `Multi-hop hover degraded or missing (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("confidence.reopen.stays-clean-after-revert", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file =
      fw === "vue"
        ? "src/confidence/InvalidateTarget.vue"
        : "src/confidence/InvalidateTarget.svelte";
    try {
      const doc = await openRelative(file);
      const editor = await vscode.window.showTextDocument(doc);
      await editor.edit((eb) => eb.insert(doc.positionAt(0), " "));
      await sleep(100);
      await revertDoc();
      await sleep(150);
      await assertCleanErrors(file);
    } catch (err) {
      await revertDoc();
      failParityGap(
        this,
        "confidence.reopen.stays-clean-after-revert",
        fw === "vue" ? "ISSUE-vue-confidence-revert" : "ISSUE-svelte-confidence-revert",
        `Revert did not restore clean diagnostics (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("confidence.battery.known-negative-files-still-error", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    // Regression battery: a single green path must not imply negatives vanished.
    const files =
      fw === "vue"
        ? [
            "src/strict/StrictUnknownProp.vue",
            "src/diagnostics/TypeNegWrongProps.vue",
            "src/slots/SlotWrongProps.vue",
          ]
        : [
            "src/strict/StrictUnknownProp.svelte",
            "src/diagnostics/TypeNegWrongProps.svelte",
            "src/slots/SnippetWrongProps.svelte",
          ];
    const failures: string[] = [];
    for (const file of files) {
      try {
        await assertHasErrorMatching(file, TYPE_MISMATCH);
      } catch (err) {
        failures.push(`${file}: ${String(err)}`);
      }
    }
    if (failures.length > 0) {
      failParityGap(
        this,
        "confidence.battery.known-negative-files-still-error",
        fw === "vue" ? "ISSUE-vue-confidence-neg-battery" : "ISSUE-svelte-confidence-neg-battery",
        `Negative battery regressions:\n${failures.join("\n")}`,
        "product-gap",
      );
    }
  });

  test("confidence.definition.never-virtual-for-daily-and-strict", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const pairs =
      fw === "vue"
        ? ([
            ["src/DailyBinding.vue", "dailyValue", 0],
            ["src/strict/StrictLeaf.vue", "label", 0],
            ["src/ide/IdeSurfaceParent.vue", "onPick", 0],
          ] as const)
        : ([
            ["src/DailyBinding.svelte", "dailyValue", 0],
            ["src/strict/StrictChild.svelte", "label", 0],
            ["src/ide/IdeSurfaceParent.svelte", "onPick", 0],
          ] as const);
    try {
      for (const [file, token, occ] of pairs) {
        const locs = await definitionsAt({ file, token, occurrence: occ });
        if (locs.length === 0) throw new Error(`no definition ${file}#${token}`);
        for (const loc of locs) {
          if (/\.(vue|svelte)\.(tsx|jsx|verter\.ts|__verter_test\.ts)/i.test(loc.uri.fsPath)) {
            throw new Error(`virtual leak from ${file}#${token}: ${loc.uri.fsPath}`);
          }
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "confidence.definition.never-virtual-for-daily-and-strict",
        fw === "vue" ? "ISSUE-vue-confidence-no-virtual" : "ISSUE-svelte-confidence-no-virtual",
        `Virtual-path definition leak (${fw}): ${String(err)}`,
        "product-gap",
      );
    }
  });
});
