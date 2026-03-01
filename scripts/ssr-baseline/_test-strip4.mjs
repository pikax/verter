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

if (result.errors?.length) {
  console.log("Errors:", result.errors.map(e => e.message || e));
  process.exit(1);
}

const body = extractSsrRenderBody(result.code);

// Find the section around _: 1 and default
const idx = body.indexOf('_: 1');
if (idx > 0) {
  console.log("=== RAW around _: 1 (first occurrence) ===");
  console.log(body.substring(Math.max(0, idx-500), idx+200));
  console.log("\n\n");
}

// Now look for the pattern: what comes before `_: 1, default:`
// In the normalized output we see `{), _: 1, default:` so let's look at what
// VDOM fallback stripping does

// Step 1: Apply just whitespace normalization
let s = body;
s = s.replace(/"use strict";?\s*/g, "");
s = s.split("\n").map(l => l.trim()).filter(l => l.length > 0).join("\n");
s = s.replace(/\s+/g, " ");

// Now find `_: 1` and show context BEFORE stripVdomFallback
const idx2 = s.indexOf('_: 1');
if (idx2 > 0) {
  console.log("=== BEFORE stripVdomFallback (around _: 1) ===");
  console.log(s.substring(Math.max(0, idx2-400), idx2+200));
}
