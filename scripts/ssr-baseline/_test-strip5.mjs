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

const vueResult = compileTemplate({
  source: descriptor.template.content,
  filename: file,
  id: file,
  ssr: true,
  compilerOptions: { mode: "module", bindingMetadata: bm },
});

const vueBody = extractSsrRenderBody(vueResult.code);
const vueNorm = normalizeForComparison(vueBody);

// Now compile with Verter
const upsertResult = host.upsert({ inputId: file, source: content, fileKind: "vue" });
const verterResult = host.getVirtualFile({
  canonicalId: upsertResult.canonicalId,
  nodeKind: { kind: "main" },
  compileProfile: { filename: path.basename(file), ssr: true, forceJs: true, sourceMap: false },
});
const verterBody = extractSsrRenderBody(verterResult.code);
const verterNorm = normalizeForComparison(verterBody);

// Find first difference
let diffIdx = -1;
for (let i = 0; i < Math.min(vueNorm.length, verterNorm.length); i++) {
  if (vueNorm[i] !== verterNorm[i]) {
    diffIdx = i;
    break;
  }
}

if (diffIdx === -1 && vueNorm.length !== verterNorm.length) {
  diffIdx = Math.min(vueNorm.length, verterNorm.length);
}

if (diffIdx === -1) {
  console.log("MATCH! No differences found.");
} else {
  console.log("First diff at index:", diffIdx);
  const ctx = 100;
  console.log("\n=== Vue (normalized) ===");
  console.log(vueNorm.substring(Math.max(0, diffIdx - ctx), diffIdx + ctx));
  console.log("\n=== Verter (normalized) ===");
  console.log(verterNorm.substring(Math.max(0, diffIdx - ctx), diffIdx + ctx));

  // Show the diff marker
  const prefix = vueNorm.substring(Math.max(0, diffIdx - ctx), diffIdx);
  console.log("\n=== Diff starts here ===");
  console.log(" ".repeat(prefix.length) + "V");
  console.log("Vue:    ..." + vueNorm.substring(diffIdx, diffIdx + 60));
  console.log("Verter: ..." + verterNorm.substring(diffIdx, diffIdx + 60));
}
