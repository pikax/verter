import { extractSsrRenderBody, normalizeForComparison, extractImports } from "./normalize.mjs";
import fs from "fs";

// Pass the mismatches JSON as the first CLI arg (e.g. ssr-full-43.json).
const __input = process.argv[2];
if (!__input) {
  console.error("usage: node _check-id-strip.mjs <mismatches.json>");
  process.exit(1);
}
const data = JSON.parse(fs.readFileSync(__input, "utf-8"));

// The normalized data already has id stripped
// Let me check: for the 680 mismatches, how many have id: in their raw output?
let withIdVue = 0;
let withIdVerter = 0;
for (const m of data.mismatches) {
  if ((m.vue || "").includes("id:")) withIdVue++;
  if ((m.verter || "").includes("id:")) withIdVerter++;
}
console.log("Mismatches with id: in Vue output:", withIdVue);
console.log("Mismatches with id: in Verter output:", withIdVerter);

// Now check: the normalized outputs already have id stripped.
// But wait - the comparison JSON stores normalized output.
// So I can't check raw vs normalized here.

// Let me look at how the compare.mjs script works differently...
// Actually the mismatches already store the normalized output.
// The id: stripping already happened before the comparison.
// To test without id stripping, I need to re-run the comparison.

console.log("\nTo test impact of removing id stripping, need to re-run comparison.");
