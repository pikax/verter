import { normalizeForComparison, extractSsrRenderBody } from "./normalize.mjs";
import fs from "fs";

const data = JSON.parse(fs.readFileSync('C:/temp/ssr-full-5.json', 'utf-8'));
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
