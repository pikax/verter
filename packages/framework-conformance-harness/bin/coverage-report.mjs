#!/usr/bin/env node
/**
 * Official-case coverage accounting CLI. Structural accounting (disposition
 * validity, no duplicates) always runs. Runner re-enumeration against the
 * pinned git checkouts runs only when BF2_VUE_SOURCE / BF2_SVELTE_SOURCE are
 * set (see src/env-paths.mjs) and is reported as SKIPPED, not silently
 * omitted, otherwise. Publishes one atomic JSON report — see
 * src/result-writer.mjs.
 */

import path from "node:path";

import {
  accountManifestStructure,
  parseCaseManifest,
  reEnumerateVueRows,
  reEnumerateSvelteRows,
  reverseEnumerateSvelteRows,
} from "../src/coverage-report.mjs";
import { assertCheckoutPinned } from "../src/checkout-pin.mjs";
import { VUE_DOMAIN, SVELTE_DOMAIN } from "../src/domain-pin.mjs";
import { oracleSourcePaths } from "../src/env-paths.mjs";
import { runAtomic } from "../src/result-writer.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";

function main() {
  const outPath = path.join(HARNESS_ROOT, ".coverage-report.json");
  const report = runAtomic(outPath, () => {
    const vueStructure = accountManifestStructure("vue-official-cases.tsv");
    const svelteStructure = accountManifestStructure("svelte-official-cases.tsv");
    if (vueStructure.rowCount !== 2003)
      throw new Error(`expected 2003 Vue rows, got ${vueStructure.rowCount}`);
    if (svelteStructure.rowCount !== 3475)
      throw new Error(`expected 3475 Svelte rows, got ${svelteStructure.rowCount}`);
    if (vueStructure.problems.length > 0)
      throw new Error(`Vue manifest problems: ${vueStructure.problems.join("; ")}`);
    if (svelteStructure.problems.length > 0)
      throw new Error(`Svelte manifest problems: ${svelteStructure.problems.join("; ")}`);

    const { vueSource, svelteSource } = oracleSourcePaths();
    let vueEnumeration = { status: "skipped", reason: "BF2_VUE_SOURCE not set" };
    let svelteEnumeration = { status: "skipped", reason: "BF2_SVELTE_SOURCE not set" };

    if (vueSource) {
      assertCheckoutPinned(vueSource, VUE_DOMAIN);
      const rows = parseCaseManifest("vue-official-cases.tsv");
      const result = reEnumerateVueRows(vueSource, rows);
      vueEnumeration = { status: "ran", ...result };
    }
    let svelteReverseEnumeration = {
      status: "skipped",
      reason: "BF2_SVELTE_SOURCE not set",
    };
    if (svelteSource) {
      assertCheckoutPinned(svelteSource, SVELTE_DOMAIN);
      const rows = parseCaseManifest("svelte-official-cases.tsv");
      const result = reEnumerateSvelteRows(svelteSource, rows);
      svelteEnumeration = { status: "ran", ...result };
      svelteReverseEnumeration = { status: "ran", ...reverseEnumerateSvelteRows(svelteSource, rows) };
    }

    return {
      vue: { structure: vueStructure, enumeration: vueEnumeration },
      svelte: {
        structure: svelteStructure,
        enumeration: svelteEnumeration,
        reverseEnumeration: svelteReverseEnumeration,
      },
    };
  });
  console.log(JSON.stringify(report, null, 2));
}

main();
