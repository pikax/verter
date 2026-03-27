import fs from "fs";

const d = JSON.parse(fs.readFileSync("C:/temp/ssr-full-style-merge2.json", "utf8"));
const mm = d.mismatches;

// For each mismatch, count number of diff lines (a proxy for how far off it is)
const fileDiffs = [];

for (const m of mm) {
  const vueLines = (m.vue || "").split("\n");
  const verterLines = (m.verter || "").split("\n");
  const maxLen = Math.max(vueLines.length, verterLines.length);

  let diffCount = 0;
  const diffPatterns = [];
  for (let i = 0; i < maxLen; i++) {
    const vl = (vueLines[i] || "").trim();
    const vrl = (verterLines[i] || "").trim();
    if (vl !== vrl) {
      diffCount++;
      if (diffPatterns.length < 3) {
        // Extract a short description of the diff
        let dp = 0;
        const ml = Math.min(vl.length, vrl.length);
        while (dp < ml && vl[dp] === vrl[dp]) dp++;
        const ctx = vl.slice(Math.max(0, dp - 20), dp + 30);
        diffPatterns.push(ctx.slice(0, 50));
      }
    }
  }

  fileDiffs.push({ file: m.file, diffCount, patterns: diffPatterns });
}

// Sort by diff count ascending
fileDiffs.sort((a, b) => a.diffCount - b.diffCount);

console.log("=== Files closest to matching (1 diff line) ===");
const oneDiff = fileDiffs.filter((f) => f.diffCount === 1);
console.log(`Count: ${oneDiff.length}`);

// Categorize the 1-diff files by pattern
const cats = {};
for (const f of oneDiff) {
  const p = f.patterns[0] || "unknown";
  let cat = "other";
  if (p.includes("elIcon") || p.includes("elT") || p.includes("ElIcon") || p.includes("ElT"))
    cat = "casing";
  else if (p.includes("_imports_")) cat = "asset-imports";
  else if (p.includes("_mergeProps")) cat = "mergeProps";
  else if (p.includes("$slots") || p.includes("Slot")) cat = "slot";
  else if (p.includes("_ssrRenderList")) cat = "v-for";
  else if (p.includes("style") || p.includes("Style")) cat = "style";
  else if (p.includes("class") || p.includes("Class")) cat = "class";
  else if (p.includes("Directive") || p.includes("directive")) cat = "directive";

  cats[cat] = (cats[cat] || 0) + 1;
}
console.log("\nCategories of 1-diff files:");
const sorted = Object.entries(cats).sort((a, b) => b[1] - a[1]);
for (const [cat, count] of sorted) console.log(`  ${cat}: ${count}`);

// Show 15 examples of 1-diff files that are NOT casing/asset
console.log("\n=== Fixable 1-diff examples ===");
let shown = 0;
for (const f of oneDiff) {
  if (shown >= 15) break;
  const p = f.patterns[0] || "";
  // Skip casing and asset imports
  if (p.includes("elIcon") || p.includes("ElIcon") || p.includes("_imports_")) continue;
  if (p.match(/[a-z][A-Z]/) && p.match(/[A-Z][a-z]/)) continue; // likely casing
  console.log(`  ${f.file}`);
  console.log(`    Pattern: ${f.patterns[0]}`);
  shown++;
}

// Also show 2-diff and 3-diff counts
const twoDiff = fileDiffs.filter((f) => f.diffCount === 2).length;
const threeDiff = fileDiffs.filter((f) => f.diffCount === 3).length;
console.log(`\n2-diff files: ${twoDiff}`);
console.log(`3-diff files: ${threeDiff}`);
