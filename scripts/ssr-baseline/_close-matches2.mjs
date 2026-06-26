import fs from "fs";

// Pass the mismatches JSON as the first CLI arg (e.g. ssr-full-style-merge2.json).
const __input = process.argv[2];
if (!__input) {
  console.error("usage: node _close-matches2.mjs <mismatches.json>");
  process.exit(1);
}
const d = JSON.parse(fs.readFileSync(__input, "utf8"));
const mm = d.mismatches;

// Group by the "category" of the first diff point
const cats = {};

for (const m of mm) {
  const vue = (m.vue || "").trim();
  const verter = (m.verter || "").trim();
  if (!vue || !verter) continue;

  // Find first diff position
  let dp = 0;
  const ml = Math.min(vue.length, verter.length);
  while (dp < ml && vue[dp] === verter[dp]) dp++;

  // Calculate how much matches before the first diff
  const matchPct = (dp / Math.max(vue.length, verter.length)) * 100;

  // Get context at diff point
  const vueCtx = vue.slice(Math.max(0, dp - 30), dp + 30);
  const verterCtx = verter.slice(Math.max(0, dp - 30), dp + 30);

  let cat = "other";
  // Detect the diff pattern
  if (dp < ml && vue[dp].toLowerCase() === verter[dp].toLowerCase()) {
    cat = "casing";
  } else if (vueCtx.includes("_imports_") || verterCtx.includes("_imports_")) {
    cat = "asset-imports";
  } else if (vueCtx.includes("_mergeProps") || verterCtx.includes("_mergeProps")) {
    cat = "mergeProps";
  } else if (vueCtx.includes("Directive") || verterCtx.includes("Directive")) {
    cat = "directive";
  } else if (vueCtx.includes("SlotInner") || verterCtx.includes("SlotInner")) {
    cat = "slotInner";
  } else if (vueCtx.includes(" style=") || verterCtx.includes(" style=")) {
    cat = "style";
  } else if (vueCtx.includes("class") || verterCtx.includes("class")) {
    cat = "class";
  } else if (vueCtx.includes("$slots") || verterCtx.includes("$slots")) {
    cat = "slot";
  } else if (vueCtx.includes("$data.") || verterCtx.includes("$data.")) {
    cat = "binding-prefix";
  }

  cats[cat] = cats[cat] || { count: 0, hi_pct: [] };
  cats[cat].count++;
  if (matchPct >= 90 && cats[cat].hi_pct.length < 3) {
    cats[cat].hi_pct.push({ file: m.file, matchPct: matchPct.toFixed(1), vueCtx, verterCtx });
  }
}

const sorted = Object.entries(cats).sort((a, b) => b[1].count - a[1].count);
console.log("=== Mismatch categories by first-diff-point ===");
for (const [cat, data] of sorted) {
  console.log(`  ${cat}: ${data.count}`);
}

// Show categories with >90% match (close to matching)
console.log("\n=== High-match-pct examples (>90% matching) ===");
for (const [cat, data] of sorted) {
  if (data.hi_pct.length > 0) {
    console.log(`\n--- ${cat} ---`);
    for (const ex of data.hi_pct) {
      console.log(`  ${ex.file} (${ex.matchPct}% match)`);
      console.log(`    Vue:    ...${ex.vueCtx}...`);
      console.log(`    Verter: ...${ex.verterCtx}...`);
    }
  }
}
