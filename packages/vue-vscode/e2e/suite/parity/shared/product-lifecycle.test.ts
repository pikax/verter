/**
 * Product DX (inlay, code actions) and light lifecycle checks for parity fixtures.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";
import { FIXTURE_NAME, sleep } from "../../../helpers";
import {
  absoluteFile,
  assertCleanErrors,
  assertHasErrorMatching,
  codeActionsForFile,
  ensureParityReady,
  inlayHintsForFile,
  openRelative,
  registerFrameworkTest,
  failParityGap,
  workspaceRoot,
} from "../../../lib/parityHarness";

function framework(): "vue" | "svelte" | "mixed" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  if (FIXTURE_NAME === "mixed-parity") return "mixed";
  return null;
}

suite(`Product and lifecycle [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = framework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "svelte" ? "src/App.svelte" : "src/App.vue");
  });

  test("shared.inlay-hints.script-region", async function () {
    const fw = framework();
    if (!fw || fw === "mixed")
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const hints = await inlayHintsForFile(file);
      // Presence is enough; empty may mean disabled — fail only if provider throws.
      if (!Array.isArray(hints)) throw new Error("inlay provider returned non-array");
      if (hints.length === 0) {
        throw new Error("no inlay hints (provider empty or settings disabled)");
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.inlay-hints.script-region",
        "ISSUE-product-inlay-hints",
        `Inlay hints not observable for ${fw}: ${String(err)}`,
        "product-gap",
      );
    }
  });

  registerFrameworkTest("vue", "shared.code-action.organize-imports-available", async function () {
    try {
      const actions = await codeActionsForFile(
        "src/features/OrganizeImports.vue",
        vscode.CodeActionKind.SourceOrganizeImports,
      );
      const hit = actions.some((a) => {
        if (!("kind" in a) || !a.kind) return false;
        return a.kind.value.startsWith("source.organizeImports");
      });
      if (!hit) throw new Error(`no organizeImports action; count=${actions.length}`);
    } catch (err) {
      failParityGap(
        this,
        "shared.code-action.organize-imports-available",
        "ISSUE-product-organize-imports",
        `Organize imports action missing: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("shared.lifecycle.external-ts-create-delete", async function () {
    const fw = framework();
    if (!fw || fw === "mixed")
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    this.timeout(60_000);
    const rel = "src/__parity_lifecycle_helper.ts";
    const abs = absoluteFile(rel);
    const consumerRel =
      fw === "vue"
        ? "src/__parity_lifecycle_consumer.vue"
        : "src/__parity_lifecycle_consumer.svelte";
    const consumerAbs = absoluteFile(consumerRel);

    const helperSource = `export function lifecyclePing(): string { return "ok"; }\n`;
    const consumerSource =
      fw === "vue"
        ? `<script setup lang="ts">\nimport { lifecyclePing } from "./__parity_lifecycle_helper";\nconst msg = lifecyclePing();\n</script>\n<template><p>{{ msg }}</p></template>\n`
        : `<script lang="ts">\n  import { lifecyclePing } from "./__parity_lifecycle_helper";\n  let msg = lifecyclePing();\n</script>\n<p>{msg}</p>\n`;

    try {
      fs.writeFileSync(abs, helperSource, "utf8");
      fs.writeFileSync(consumerAbs, consumerSource, "utf8");
      // Allow VFS / scanner to observe new files.
      await sleep(800);
      await openRelative(consumerRel);
      await assertCleanErrors(consumerRel);

      fs.unlinkSync(abs);
      await sleep(800);
      await assertHasErrorMatching(consumerRel, /2307|Cannot find module|lifecyclePing|module/i);
    } catch (err) {
      failParityGap(
        this,
        "shared.lifecycle.external-ts-create-delete",
        "ISSUE-lifecycle-external-ts",
        `External create/delete invalidation failed for ${fw}: ${String(err)}`,
      );
    } finally {
      try {
        if (fs.existsSync(abs)) fs.unlinkSync(abs);
      } catch {
        /* best-effort */
      }
      try {
        if (fs.existsSync(consumerAbs)) fs.unlinkSync(consumerAbs);
      } catch {
        /* best-effort */
      }
    }
  });

  test("shared.product.emmet-or-unsupported", async function () {
    const fw = framework();
    if (!fw || fw === "mixed")
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    // Emmet is editor-integrated; Verter may not own it. Record explicit gap if no expansion path.
    try {
      const commands = await vscode.commands.getCommands(true);
      const emmet = commands.filter((c) => /emmet/i.test(c));
      if (emmet.length === 0) {
        throw new Error("no emmet commands registered in this host");
      }
    } catch (err) {
      failParityGap(
        this,
        "shared.product.emmet-or-unsupported",
        "ISSUE-product-emmet",
        `Emmet not available/owned for ${fw}: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("shared.workspace-root.present", function () {
    const fw = framework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const root = workspaceRoot();
    if (!fs.existsSync(path.join(root, "package.json"))) {
      throw new Error(`fixture root missing package.json: ${root}`);
    }
  });
});
