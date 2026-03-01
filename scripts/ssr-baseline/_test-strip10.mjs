import { extractSsrRenderBody } from "./normalize.mjs";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, "../..");
const { parse, compileScript, compileTemplate } = require(
  require.resolve("@vue/compiler-sfc", { paths: [path.join(ROOT, "node_modules/.pnpm")] }),
);

// Use the full compare.mjs file set; pick a few mismatched files from the JSON report
const jsonReport = JSON.parse(fs.readFileSync("C:/temp/ssr-full.json", "utf-8"));

// Look for mismatches where vue contains "), _: 1, default:"
let count = 0;
for (const m of jsonReport.mismatches) {
  if (m.vue && m.vue.includes('), _: 1,')) {
    count++;
    if (count <= 3) {
      console.log(`\nFile: ${m.file}`);
      // Find the context
      const idx = m.vue.indexOf('), _: 1,');
      console.log("Vue around issue:");
      console.log(m.vue.substring(Math.max(0, idx - 100), idx + 80));

      // Check if verter has the same region
      const verterIdx = m.verter.indexOf('), _: 1,');
      if (verterIdx !== -1) {
        console.log("\nVerter also has '), _: 1,' at", verterIdx);
      } else {
        console.log("\nVerter does NOT have '), _: 1,'");
        // Find what verter has instead near the same position
        // Look for the equivalent context before the first diff
        let diffAt = 0;
        for (let i = 0; i < Math.min(m.vue.length, m.verter.length); i++) {
          if (m.vue[i] !== m.verter[i]) { diffAt = i; break; }
        }
        console.log("First diff at char", diffAt);
        console.log("Vue:    ..." + m.vue.substring(Math.max(0, diffAt - 20), diffAt + 40) + "...");
        console.log("Verter: ..." + m.verter.substring(Math.max(0, diffAt - 20), diffAt + 40) + "...");
      }
    }
  }
}
console.log(`\nTotal mismatches with '), _: 1,': ${count}`);

// Also check: how many have '{)' in the vue normalized output?
let braceParenCount = 0;
for (const m of jsonReport.mismatches) {
  if (m.vue && m.vue.includes('{)')) braceParenCount++;
}
console.log(`Mismatches with '{)' in Vue: ${braceParenCount}`);

// Check verter
let verterBraceParenCount = 0;
for (const m of jsonReport.mismatches) {
  if (m.verter && m.verter.includes('{)')) verterBraceParenCount++;
}
console.log(`Mismatches with '{)' in Verter: ${verterBraceParenCount}`);
