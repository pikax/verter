import { extractSsrRenderBody, normalizeForComparison, extractImports } from "./normalize.mjs";
import fs from "fs";

const data = JSON.parse(fs.readFileSync("C:/temp/ssr-full-43.json", "utf-8"));

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
