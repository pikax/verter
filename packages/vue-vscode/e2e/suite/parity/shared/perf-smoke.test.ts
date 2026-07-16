/**
 * Lightweight performance smoke: completion/hover should answer within budgets.
 * Budget failures remain red product gaps.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  ensureParityReady,
  openRelative,
  pollUntil,
  failParityGap,
  tokenPosition,
} from "../../../lib/parityHarness";
import * as vscode from "vscode";

function framework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

const HOVER_BUDGET_MS = 2_500;
const COMPLETION_BUDGET_MS = 3_000;

suite(`Performance smoke [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("shared.perf.hover.warm-budget", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const doc = await openRelative(file);
      const pos = tokenPosition(doc, { file, token: "dailyValue", occurrence: 3 });
      // Warm once
      await pollUntil(
        "warm hover",
        async () =>
          (await vscode.commands.executeCommand<vscode.Hover[]>(
            "vscode.executeHoverProvider",
            doc.uri,
            pos,
          )) ?? [],
        (r) => r.length > 0,
      );
      const t0 = Date.now();
      const hovers =
        (await vscode.commands.executeCommand<vscode.Hover[]>(
          "vscode.executeHoverProvider",
          doc.uri,
          pos,
        )) ?? [];
      const ms = Date.now() - t0;
      if (hovers.length === 0) throw new Error("empty hover");
      if (ms > HOVER_BUDGET_MS) throw new Error(`hover ${ms}ms > ${HOVER_BUDGET_MS}ms`);
    } catch (err) {
      failParityGap(
        this,
        "shared.perf.hover.warm-budget",
        "ISSUE-perf-hover",
        String(err),
        "product-gap",
      );
    }
  });

  test("shared.perf.completion.warm-budget", async function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const doc = await openRelative(file);
      const pos = tokenPosition(doc, { file, token: "dailyValue", occurrence: 3, caretOffset: 0 });
      await pollUntil(
        "warm completion",
        async () =>
          (await vscode.commands.executeCommand<vscode.CompletionList>(
            "vscode.executeCompletionItemProvider",
            doc.uri,
            pos,
          )) ?? { items: [] },
        (r) => (r.items?.length ?? 0) > 0,
      );
      const t0 = Date.now();
      const list = (await vscode.commands.executeCommand<vscode.CompletionList>(
        "vscode.executeCompletionItemProvider",
        doc.uri,
        pos,
      )) ?? { items: [] };
      const ms = Date.now() - t0;
      if ((list.items?.length ?? 0) === 0) throw new Error("empty completion");
      if (ms > COMPLETION_BUDGET_MS) {
        throw new Error(`completion ${ms}ms > ${COMPLETION_BUDGET_MS}ms`);
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.perf.completion.warm-budget",
        "ISSUE-perf-completion",
        String(err),
        "product-gap",
      );
    }
  });
});
