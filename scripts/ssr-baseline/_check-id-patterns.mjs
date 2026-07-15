import fs from "fs";
// Pass the mismatches JSON as the first CLI arg (e.g. ssr-full-43.json).
const __input = process.argv[2];
if (!__input) {
  console.error("usage: node _check-id-patterns.mjs <mismatches.json>");
  process.exit(1);
}
const data = JSON.parse(fs.readFileSync(__input, "utf-8"));

// Check what id: patterns exist in the NORMALIZED output
const patterns = {};
for (const m of data.mismatches) {
  for (const [key, output] of [
    ["vue", m.vue],
    ["verter", m.verter],
  ]) {
    const matches = (output || "").matchAll(/\bid:\s*([^,}\s]+)/g);
    for (const match of matches) {
      const pat = match[1].substring(0, 30);
      const k = `${key}: id: ${pat}`;
      patterns[k] = (patterns[k] || 0) + 1;
    }
  }
}
const sorted = Object.entries(patterns).sort((a, b) => b[1] - a[1]);
for (const [k, c] of sorted.slice(0, 30)) {
  console.log(c.toString().padStart(4), k);
}
