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

// Use one of the actual mergeProps mismatch files
const VERTER_TEST_REPOS = process.env.VERTER_TEST_REPOS;
if (!VERTER_TEST_REPOS) {
  console.error("Set VERTER_TEST_REPOS env var");
  process.exit(1);
}
const file = path.join(
  VERTER_TEST_REPOS,
  "balancer-frontend-v2/src/components/contextual/pages/vebal/MultiVoting/VoteInput.vue",
);
if (!fs.existsSync(file)) {
  console.log("File not found:", file);
  process.exit(1);
}

const content = fs.readFileSync(file, "utf-8");
const filename = path.basename(file);

// Compile with Vue
const { descriptor } = parse(content, { filename });
let bm = {};
if (descriptor.script || descriptor.scriptSetup) {
  try {
    const sr = compileScript(descriptor, { id: filename, inlineTemplate: false });
    bm = sr.bindings || {};
  } catch (e) {
    console.log("Vue compileScript error:", e.message);
  }
}

const vueResult = compileTemplate({
  source: descriptor.template.content,
  filename,
  id: filename,
  ssr: true,
  compilerOptions: { mode: "module", bindingMetadata: bm },
});
const vueBody = extractSsrRenderBody(vueResult.code);
const vueNorm = normalizeForComparison(vueBody);

// Compile with Verter
const upsertResult = host.upsert({ inputId: file, source: content, fileKind: "vue" });
const verterResult = host.getVirtualFile({
  canonicalId: upsertResult.canonicalId,
  nodeKind: { kind: "main" },
  compileProfile: { filename, ssr: true, forceJs: true, sourceMap: false },
});
const verterBody = extractSsrRenderBody(verterResult.code);
const verterNorm = normalizeForComparison(verterBody);

// Find first diff
let diffAt = 0;
for (let i = 0; i < Math.min(vueNorm.length, verterNorm.length); i++) {
  if (vueNorm[i] !== verterNorm[i]) {
    diffAt = i;
    break;
  }
}

console.log("First diff at char", diffAt);
console.log("Vue:    ..." + vueNorm.substring(Math.max(0, diffAt - 60), diffAt + 80) + "...");
console.log("Verter: ..." + verterNorm.substring(Math.max(0, diffAt - 60), diffAt + 80) + "...");

// Also show the raw context around the diff
// Find corresponding position in raw vue body
const beforeStr = vueNorm.substring(Math.max(0, diffAt - 30), diffAt);
const rawIdx = vueBody?.indexOf(beforeStr.replace(/\s+/g, " "));
if (rawIdx !== -1 && rawIdx !== undefined) {
  console.log("\n--- Vue raw around diff ---");
  console.log(vueBody.substring(Math.max(0, rawIdx), rawIdx + 300));
}
