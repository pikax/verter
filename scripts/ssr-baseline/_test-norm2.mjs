import { normalizeForComparison, extractSsrRenderBody } from "./normalize.mjs";
import fs from "fs";

// Pass the mismatches JSON as the first CLI arg (e.g. ssr-full-5.json).
const __input = process.argv[2];
if (!__input) {
  console.error("usage: node _test-norm2.mjs <mismatches.json>");
  process.exit(1);
}
const data = JSON.parse(fs.readFileSync(__input, "utf-8"));
const info = data.mismatches["1"];

// Show raw vs normalized for Vue
console.log("Vue raw (first 300):");
console.log(info.vue.substring(0, 300));
console.log("\nVue normalized (first 300):");
console.log(normalizeForComparison(info.vue).substring(0, 300));
console.log("\nVerter raw (first 300):");
console.log(info.verter.substring(0, 300));
console.log("\nVerter normalized (first 300):");
console.log(normalizeForComparison(info.verter).substring(0, 300));
