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

// Use the actual VoteInput.vue file
const VERTER_TEST_REPOS = process.env.VERTER_TEST_REPOS;
if (!VERTER_TEST_REPOS) { console.error('Set VERTER_TEST_REPOS env var'); process.exit(1); }
const file = path.join(VERTER_TEST_REPOS, "balancer-frontend-v2/src/components/contextual/pages/vebal/MultiVoting/VoteInput.vue");
const content = fs.readFileSync(file, "utf-8");
const filename = path.basename(file);

// Vue compile
const { descriptor } = parse(content, { filename });
let bm = {};
if (descriptor.script || descriptor.scriptSetup) {
  try {
    const sr = compileScript(descriptor, { id: filename, inlineTemplate: false });
    bm = sr.bindings || {};
    console.log("Vue bindings:", JSON.stringify(bm));
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

// Verter compile
const upsertResult = host.upsert({ inputId: file, source: content, fileKind: "vue" });
const verterResult = host.getVirtualFile({
  canonicalId: upsertResult.canonicalId,
  nodeKind: { kind: "main" },
  compileProfile: { filename, ssr: true, forceJs: true, sourceMap: false },
});
const verterBody = extractSsrRenderBody(verterResult.code);

// Show the BalTextInput _ssrRenderComponent call for both
function findComponentCall(body, compName) {
  const idx = body.indexOf(`_ssrRenderComponent(${compName}`);
  if (idx === -1) return null;
  // Find the end of the call (balanced parens)
  let depth = 0;
  let start = idx;
  for (let i = idx; i < body.length; i++) {
    if (body[i] === '(') depth++;
    else if (body[i] === ')') {
      depth--;
      if (depth === 0) return body.substring(start, i + 1);
    }
  }
  return null;
}

// For Vue, component may be _component_BalTextInput or _ctx["BalTextInput"]
const vueCall = findComponentCall(vueBody, '_component_BalTextInput') || findComponentCall(vueBody, '_ctx["BalTextInput"]');
const verterCall = findComponentCall(verterBody, '$setup["BalTextInput"]') || findComponentCall(verterBody, '_ctx["BalTextInput"]') || findComponentCall(verterBody, '_component_BalTextInput');

console.log("\n=== Vue BalTextInput call ===");
console.log(vueCall?.substring(0, 400));

console.log("\n=== Verter BalTextInput call ===");
console.log(verterCall?.substring(0, 400));
