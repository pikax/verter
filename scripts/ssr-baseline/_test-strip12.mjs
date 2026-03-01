import { extractSsrRenderBody, normalizeForComparison } from "./normalize.mjs";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, "../..");
const { parse, compileScript, compileTemplate } = require(
  require.resolve("@vue/compiler-sfc", { paths: [path.join(ROOT, "node_modules/.pnpm")] }),
);

const { VerterHost } = require(path.join(ROOT, "packages/native/index.js"));
const host = new VerterHost({ devMode: false, analysisLevel: "none" });

// Read the JSON report and pick the most common mismatch patterns
const jsonReport = JSON.parse(fs.readFileSync("C:/temp/ssr-full.json", "utf-8"));

// Categorize mismatches more precisely
const categories = {};
for (const m of jsonReport.mismatches) {
  const vue = m.vue;
  const verter = m.verter;
  if (!vue || !verter) continue;

  // Find first diff
  let diffAt = 0;
  for (let i = 0; i < Math.min(vue.length, verter.length); i++) {
    if (vue[i] !== verter[i]) { diffAt = i; break; }
  }
  if (diffAt === 0 && vue[0] === verter[0]) {
    diffAt = Math.min(vue.length, verter.length);
  }

  // Get context around the diff
  const vueCtx = vue.substring(diffAt, diffAt + 60);
  const verterCtx = verter.substring(diffAt, diffAt + 60);

  // Categorize by what the diff looks like
  let cat;
  if (vueCtx.startsWith('), _: 1,')) cat = 'residual-paren-before-slot-flag';
  else if (vueCtx.startsWith('_mergeProps') || verterCtx.startsWith('_mergeProps')) cat = 'mergeProps';
  else if (vueCtx.startsWith('_ssrRenderAttr') || verterCtx.startsWith('_ssrRenderAttr')) cat = 'ssrRenderAttr';
  else if (vueCtx.includes('_imports_') || verterCtx.includes('_imports_')) cat = 'asset-imports';
  else if (vueCtx.startsWith('_ctx.$attrs') || verterCtx.startsWith('_ctx.$attrs')) cat = 'ctx-attrs';
  else if (vueCtx.includes('Teleport') || verterCtx.includes('Teleport')) cat = 'teleport';
  else if (vueCtx.includes('KeepAlive') || verterCtx.includes('KeepAlive')) cat = 'keepalive';
  else if (vueCtx.includes('_withDirectives') || verterCtx.includes('_withDirectives')) cat = 'directives';
  else if (vueCtx.includes('v-model') || verterCtx.includes('v-model') || vueCtx.includes('modelValue')) cat = 'v-model';
  else if (vueCtx.startsWith('`') || verterCtx.startsWith('`') || vueCtx.startsWith('_push(') || verterCtx.startsWith('_push(')) {
    // Look more carefully
    if (vueCtx.startsWith('_push(_ssrRender') || verterCtx.startsWith('_push(_ssrRender')) cat = 'push-component-diff';
    else cat = 'push-content-diff';
  }
  else if (vueCtx.includes('_createVNode') || verterCtx.includes('_createVNode')) cat = 'vdom-vnode-diff';
  else if (vueCtx.startsWith('{') || verterCtx.startsWith('{')) cat = 'object-structure';
  else cat = 'other:' + vueCtx.substring(0, 20).replace(/\s+/g, '_');

  if (!categories[cat]) categories[cat] = [];
  categories[cat].push({ file: m.file, vueCtx: vueCtx.substring(0, 40), verterCtx: verterCtx.substring(0, 40) });
}

// Print sorted by count
const sorted = Object.entries(categories).sort((a, b) => b[1].length - a[1].length);
for (const [cat, items] of sorted) {
  console.log(`\n${cat}: ${items.length}`);
  // Show first 2 examples
  for (let i = 0; i < Math.min(2, items.length); i++) {
    const it = items[i];
    console.log(`  ${it.file}`);
    console.log(`    Vue:    ${it.vueCtx}`);
    console.log(`    Verter: ${it.verterCtx}`);
  }
}
