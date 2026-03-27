import fs from "fs";

const d = JSON.parse(fs.readFileSync("C:/temp/ssr-full-style-merge2.json", "utf8"));
const mm = d.mismatches;

// Show mergeProps mismatch sub-patterns
const subcats = {};
const examples = {};
let count = 0;

for (const m of mm) {
  const vueLines = (m.vue || "").split("\n");
  const verterLines = (m.verter || "").split("\n");
  const maxLen = Math.max(vueLines.length, verterLines.length);

  const diffs = [];
  for (let i = 0; i < maxLen; i++) {
    const vl = (vueLines[i] || "").trim();
    const vrl = (verterLines[i] || "").trim();
    if (vl !== vrl) diffs.push({ vue: vl, verter: vrl });
  }
  if (diffs.length === 0) continue;

  // Check if mergeProps-related
  const allDiffText = diffs.map((d) => d.vue + " ||| " + d.verter).join("\n");
  if (!allDiffText.includes("_mergeProps") && !allDiffText.includes("mergeProps")) continue;

  // Skip casing-only diffs
  const d0 = diffs[0];
  let dp = 0;
  const ml = Math.min(d0.vue.length, d0.verter.length);
  while (dp < ml && d0.vue[dp] === d0.verter[dp]) dp++;
  if (dp < ml && d0.vue[dp].toLowerCase() === d0.verter[dp].toLowerCase()) continue;

  // Sub-categorize
  let subcat = "other";
  const vueCtx = d0.vue.slice(Math.max(0, dp - 40), dp + 80);
  const verterCtx = d0.verter.slice(Math.max(0, dp - 40), dp + 80);

  if (vueCtx.includes("_attrs") && !verterCtx.includes("_attrs")) {
    subcat = "missing-_attrs";
  } else if (!vueCtx.includes("_mergeProps") && verterCtx.includes("_mergeProps")) {
    subcat = "extra-mergeProps";
  } else if (vueCtx.includes("_mergeProps") && !verterCtx.includes("_mergeProps")) {
    subcat = "missing-mergeProps";
  } else if (vueCtx.includes("_mergeProps") && verterCtx.includes("_mergeProps")) {
    subcat = "mergeProps-args-diff";
  }

  subcats[subcat] = (subcats[subcat] || 0) + 1;
  if (!examples[subcat]) examples[subcat] = [];
  if (examples[subcat].length < 3) {
    examples[subcat].push({
      file: m.file,
      vueCtx: vueCtx.slice(0, 120),
      verterCtx: verterCtx.slice(0, 120),
    });
  }
  count++;
}

console.log(`Total mergeProps mismatches: ${count}`);
const sorted = Object.entries(subcats).sort((a, b) => b[1] - a[1]);
for (const [cat, cnt] of sorted) {
  console.log(`  ${cat}: ${cnt}`);
}

console.log("\n=== Examples ===");
for (const [cat, cnt] of sorted) {
  console.log(`\n--- ${cat} (${cnt}) ---`);
  for (const ex of examples[cat]) {
    console.log(`  File: ${ex.file}`);
    console.log(`    Vue:    ...${ex.vueCtx}...`);
    console.log(`    Verter: ...${ex.verterCtx}...`);
  }
}
