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

// Step 1: Apply whitespace normalization only
let s = body;
s = s.replace(/"use strict";?\s*/g, "");
s = s.split("\n").map(l => l.trim()).filter(l => l.length > 0).join("\n");
s = s.replace(/\s+/g, " ");

// Find BalCard with noBorder
const searchIdx = s.indexOf('noBorder');
if (searchIdx !== -1) {
  // Show the raw (whitespace-normalized) before any stripping
  console.log("=== Raw (whitespace-normalized) around first BalCard with noBorder ===");
  console.log(s.substring(Math.max(0, searchIdx - 200), searchIdx + 500));
  console.log("\n\n");
}

// Now let's look at what the slot object looks like for this component.
// The structure should be: _ssrRenderComponent(comp, props, { slotName: _withCtx(...), _: 1 }, ...)
// Find the slot object opening {
const compIdx = s.indexOf('_ssrRenderComponent(_ctx["BalCard"]');
if (compIdx !== -1) {
  // Find the third argument (the slot object)
  // Pattern: _ssrRenderComponent(comp, props, {slotObject}, ...)
  console.log("=== Around _ssrRenderComponent(_ctx['BalCard']) ===");
  console.log(s.substring(compIdx, compIdx + 1000));
}
