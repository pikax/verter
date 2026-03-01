/**
 * Report formatting for SSR baseline comparison.
 * Console summary, mismatch pattern grouping, and JSON output.
 */

import fs from "node:fs";

/**
 * Mismatch pattern detection heuristics.
 * Each pattern checks for keywords in the Vue/Verter diff to categorize
 * the type of SSR mismatch.
 */
const PATTERN_RULES = [
  {
    name: "v-for rendering",
    test: (vue, verter) =>
      has(vue, "_ssrRenderList") !== has(verter, "_ssrRenderList"),
  },
  {
    name: "Component rendering",
    test: (vue, verter) =>
      has(vue, "_ssrRenderComponent") !== has(verter, "_ssrRenderComponent"),
  },
  {
    name: "Slot rendering",
    test: (vue, verter) =>
      has(vue, "_ssrRenderSlot") !== has(verter, "_ssrRenderSlot") ||
      has(vue, "_withCtx") !== has(verter, "_withCtx"),
  },
  {
    name: "v-show handling",
    test: (vue, verter) =>
      (has(vue, 'display') || has(vue, "v-show")) !==
      (has(verter, 'display') || has(verter, "v-show")),
  },
  {
    name: "Teleport SSR",
    test: (vue, verter) =>
      has(vue, "_ssrRenderTeleport") !== has(verter, "_ssrRenderTeleport"),
  },
  {
    name: "v-model SSR",
    test: (vue, verter) =>
      has(vue, "_ssrGetDynamicModelProps") !==
      has(verter, "_ssrGetDynamicModelProps"),
  },
  {
    name: "Suspense SSR",
    test: (vue, verter) =>
      has(vue, "_ssrRenderSuspense") !== has(verter, "_ssrRenderSuspense"),
  },
  {
    name: "Attribute rendering",
    test: (vue, verter) =>
      has(vue, "_ssrRenderAttrs") !== has(verter, "_ssrRenderAttrs") ||
      has(vue, "_ssrRenderAttr") !== has(verter, "_ssrRenderAttr"),
  },
  {
    name: "Class/style rendering",
    test: (vue, verter) =>
      has(vue, "_ssrRenderClass") !== has(verter, "_ssrRenderClass") ||
      has(vue, "_ssrRenderStyle") !== has(verter, "_ssrRenderStyle"),
  },
];

function has(str, keyword) {
  return str != null && str.includes(keyword);
}

/**
 * Detect the mismatch pattern for a given Vue/Verter diff.
 * Returns the first matching pattern name, or "Other" if none match.
 */
export function detectPattern(vue, verter) {
  for (const rule of PATTERN_RULES) {
    if (rule.test(vue, verter)) return rule.name;
  }
  return "Other";
}

/**
 * Print console summary of SSR comparison results.
 *
 * @param {object} stats - Comparison statistics
 * @param {object[]} mismatches - Array of mismatch entries
 * @param {object} errors - { vue: [...], verter: [...] }
 * @param {number} elapsed - Elapsed time in seconds
 */
export function printSummary(stats, mismatches, errors, elapsed) {
  const pct = (n) =>
    stats.total > 0 ? ((n / stats.total) * 100).toFixed(1) : "0.0";

  console.log(`\nSSR Baseline Comparison`);
  console.log("=".repeat(50));
  console.log(`Total files:       ${fmt(stats.total)}`);
  console.log(
    `Matches:           ${fmt(stats.matches).padEnd(8)} (${pct(stats.matches)}%)`,
  );
  console.log(
    `Mismatches:        ${fmt(stats.mismatches).padEnd(8)} (${pct(stats.mismatches)}%)`,
  );
  console.log(
    `Vue errors:        ${fmt(stats.vueErrors).padEnd(8)} (${pct(stats.vueErrors)}%)`,
  );
  console.log(
    `Verter errors:     ${fmt(stats.verterErrors).padEnd(8)} (${pct(stats.verterErrors)}%)`,
  );
  console.log(
    `Both errors:       ${fmt(stats.bothErrors).padEnd(8)} (${pct(stats.bothErrors)}%)`,
  );
  console.log(
    `No template:       ${fmt(stats.noTemplate).padEnd(8)} (${pct(stats.noTemplate)}%)`,
  );
  console.log(`Time:              ${elapsed}s`);

  // Pattern breakdown
  if (mismatches.length > 0) {
    const patternCounts = {};
    for (const m of mismatches) {
      patternCounts[m.pattern] = (patternCounts[m.pattern] || 0) + 1;
    }
    const sorted = Object.entries(patternCounts).sort((a, b) => b[1] - a[1]);

    console.log(`\nTop mismatch patterns:`);
    for (const [name, count] of sorted) {
      console.log(`  ${count.toString().padStart(6)}  ${name}`);
    }
  }

  // Error groups
  printErrorGroups("Vue", errors.vue);
  printErrorGroups("Verter", errors.verter);
}

function printErrorGroups(label, errorList) {
  if (errorList.length === 0) return;

  const groups = {};
  for (const e of errorList) {
    const key = e.error.slice(0, 80);
    groups[key] = (groups[key] || 0) + 1;
  }
  const sorted = Object.entries(groups).sort((a, b) => b[1] - a[1]);

  console.log(`\n${label} error groups (${errorList.length} files):`);
  for (const [msg, count] of sorted.slice(0, 15)) {
    console.log(`  (${count}x) ${msg}`);
  }
  if (sorted.length > 15)
    console.log(`  ... and ${sorted.length - 15} more groups`);
}

function fmt(n) {
  return n.toLocaleString("en-US");
}

/**
 * Write JSON report to file.
 *
 * @param {string} outputPath - Path to write JSON
 * @param {object} stats - Comparison statistics
 * @param {object[]} mismatches - Array of mismatch entries
 * @param {object} errors - { vue: [...], verter: [...] }
 */
export function writeJsonReport(outputPath, stats, mismatches, errors) {
  const patternCounts = {};
  for (const m of mismatches) {
    patternCounts[m.pattern] = (patternCounts[m.pattern] || 0) + 1;
  }

  const report = {
    timestamp: new Date().toISOString(),
    summary: stats,
    patterns: patternCounts,
    mismatches: mismatches.map((m) => ({
      file: m.file,
      pattern: m.pattern,
      vue: m.vue,
      verter: m.verter,
    })),
    errors: {
      vue: errors.vue.slice(0, 200),
      verter: errors.verter.slice(0, 200),
    },
  };

  fs.writeFileSync(outputPath, JSON.stringify(report, null, 2));
  console.log(`\nJSON report written to: ${outputPath}`);
}
