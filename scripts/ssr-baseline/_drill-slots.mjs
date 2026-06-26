import fs from "fs";

// Pass the mismatches JSON as the first CLI arg (e.g. ssr-full-style-merge2.json).
const __input = process.argv[2];
if (!__input) {
  console.error("usage: node _drill-slots.mjs <mismatches.json>");
  process.exit(1);
}
const d = JSON.parse(fs.readFileSync(__input, "utf8"));
const mm = d.mismatches;

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
    if (vl !== vrl) diffs.push({ vue: vl, verter: vrl });
  }
  if (diffs.length === 0) continue;

  const allDiffText = diffs.map((d) => d.vue + " ||| " + d.verter).join("\n");
  if (!allDiffText.includes("$slots")) continue;
  // Skip casing
  const d0 = diffs[0];
  let dp = 0;
  const ml = Math.min(d0.vue.length, d0.verter.length);
  while (dp < ml && d0.vue[dp] === d0.verter[dp]) dp++;
  if (dp < ml && d0.vue[dp].toLowerCase() === d0.verter[dp].toLowerCase()) continue;

  console.log(`\n=== ${m.file} ===`);
  const vueCtx = d0.vue.slice(Math.max(0, dp - 60), dp + 100);
  const verterCtx = d0.verter.slice(Math.max(0, dp - 60), dp + 100);
  console.log(`  Vue:    ...${vueCtx}...`);
  console.log(`  Verter: ...${verterCtx}...`);
  count++;
}
console.log(`\nShowed ${count} examples`);
