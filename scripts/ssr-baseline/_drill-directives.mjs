import fs from "fs";

const d = JSON.parse(fs.readFileSync("C:/temp/ssr-full-vmodel-merge.json", "utf8"));
const mm = d.mismatches;

// Show directive mismatch details
let count = 0;
for (const m of mm) {
  if (count >= 10) break;

  const vueLines = (m.vue || "").split("\n");
  const verterLines = (m.verter || "").split("\n");
  const maxLen = Math.max(vueLines.length, verterLines.length);

  const diffs = [];
  for (let i = 0; i < maxLen; i++) {
    const vl = (vueLines[i] || "").trim();
    const vrl = (verterLines[i] || "").trim();
    if (vl !== vrl) {
      diffs.push({ vue: vl, verter: vrl, lineIdx: i });
    }
  }
  if (diffs.length === 0) continue;

  // Check if any diff involves directive-related content
  const allDiffText = diffs.map((d) => d.vue + " ||| " + d.verter).join("\n");
  if (
    !allDiffText.includes("_ssrGetDirectiveProps") &&
    !allDiffText.includes("_resolveDirective") &&
    !allDiffText.includes("Directive")
  )
    continue;

  // Skip if it's element-plus casing issue
  const firstDiff = diffs[0];
  let dp = 0;
  const ml = Math.min(firstDiff.vue.length, firstDiff.verter.length);
  while (dp < ml && firstDiff.vue[dp] === firstDiff.verter[dp]) dp++;
  if (dp < ml && firstDiff.vue[dp].toLowerCase() === firstDiff.verter[dp].toLowerCase()) continue;

  console.log(`\n=== ${m.file} ===`);
  for (const diff of diffs.slice(0, 3)) {
    let dp2 = 0;
    const ml2 = Math.min(diff.vue.length, diff.verter.length);
    while (dp2 < ml2 && diff.vue[dp2] === diff.verter[dp2]) dp2++;
    const start = Math.max(0, dp2 - 60);
    const end = dp2 + 80;
    console.log(`  Line ${diff.lineIdx}:`);
    console.log(`    Vue:    ...${diff.vue.slice(start, end)}...`);
    console.log(`    Verter: ...${diff.verter.slice(start, end)}...`);
  }
  count++;
}

console.log(`\nShowed ${count} examples`);
