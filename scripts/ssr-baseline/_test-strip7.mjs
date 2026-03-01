import { extractSsrRenderBody, normalizeForComparison } from "./normalize.mjs";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, "../..");
const { parse, compileScript, compileTemplate } = require(
  require.resolve("@vue/compiler-sfc", { paths: [path.join(ROOT, "node_modules/.pnpm")] }),
);

const file = "D:\\dev\\personal\\verter\\scripts\\compare-tsx-output\\github_verter-test-repos_balancer-frontend-v2_src_components_cards_CreatePool_ChooseWeights.vue\\source.vue";
const content = fs.readFileSync(file, 'utf-8');
const { descriptor } = parse(content, { filename: file });

let bm = {};
if (descriptor.script || descriptor.scriptSetup) {
  try {
    const sr = compileScript(descriptor, { id: file, inlineTemplate: false });
    bm = sr.bindings || {};
  } catch {}
}

const result = compileTemplate({
  source: descriptor.template.content,
  filename: file,
  id: file,
  ssr: true,
  compilerOptions: { mode: "module", bindingMetadata: bm },
});

const body = extractSsrRenderBody(result.code);

// Apply only whitespace normalization
let s = body;
s = s.replace(/"use strict";?\s*/g, "");
s = s.split("\n").map(l => l.trim()).filter(l => l.length > 0).join("\n");
s = s.replace(/\s+/g, " ");

// Find BalCard noBorder and show the full slot object
const idx = s.indexOf('noBorder: "", shadow: "xl"');
if (idx === -1) {
  // Try the other order
  const idx2 = s.indexOf('shadow: "xl", noBorder');
  if (idx2 !== -1) {
    console.log("Found with shadow first at", idx2);
    const slotStart = s.indexOf('{', s.indexOf('}', idx2));
    console.log(s.substring(idx2, idx2 + 2000));
  }
} else {
  console.log("Found noBorder, shadow at", idx);
  // Show the slot object that follows
  // Pattern: _ssrRenderComponent(comp, {props}, {slots}, _parent)
  // Find the slot object opening { after the props }
  let j = idx;
  // Find closing } of props
  let depth = 0;
  // Go back to find opening {
  let k = idx - 1;
  while (k >= 0 && s[k] !== '{') k--;
  depth = 1;
  j = k + 1;
  while (j < s.length && depth > 0) {
    if (s[j] === '{') depth++;
    else if (s[j] === '}') depth--;
    j++;
  }
  // j is now past the closing } of props. Skip comma+space to get to slots
  while (j < s.length && (s[j] === ',' || s[j] === ' ')) j++;

  console.log("=== Slot object for BalCard (first 2000 chars) ===");
  console.log(s.substring(j, j + 2000));
}
