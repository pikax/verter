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

const result = compileTemplate({
  source: descriptor.template.content,
  filename: file,
  id: file,
  ssr: true,
  compilerOptions: { mode: "module", bindingMetadata: bm },
});

const body = extractSsrRenderBody(result.code);

// Now compile with Verter
const upsertResult = host.upsert({ inputId: file, source: content, fileKind: "vue" });
const verterResult = host.getVirtualFile({
  canonicalId: upsertResult.canonicalId,
  nodeKind: { kind: "main" },
  compileProfile: { filename: path.basename(file), ssr: true, forceJs: true, sourceMap: false },
});
const verterBody = extractSsrRenderBody(verterResult.code);

// Apply only whitespace normalization to both
function wsNorm(s) {
  s = s.replace(/"use strict";?\s*/g, "");
  s = s.split("\n").map(l => l.trim()).filter(l => l.length > 0).join("\n");
  s = s.replace(/\s+/g, " ");
  return s;
}

const vueWs = wsNorm(body);
const verterWs = wsNorm(verterBody);

// Now find the first BalCard slot object in both and compare
function extractSlotObj(s, searchAfter) {
  const idx = s.indexOf(searchAfter);
  if (idx === -1) return null;
  // Find after the search string: go to the slot object (3rd arg of _ssrRenderComponent)
  // Start from the component name, skip past props to the slot object
  let j = idx;
  // Find the slot {
  // props: { shadow: "xl", noBorder: "" } → next comes slot: { default: _withCtx(...)... }
  // After noBorder: "" }, we need to find the next {
  j = s.indexOf('}', j);
  if (j === -1) return null;
  j++; // past } of props
  while (j < s.length && (s[j] === ',' || s[j] === ' ')) j++;
  if (s[j] !== '{') return null;

  // Find balanced } for the slot object
  let depth = 0;
  let start = j;
  while (j < s.length) {
    if (s[j] === '{') depth++;
    else if (s[j] === '}') {
      depth--;
      if (depth === 0) { j++; break; }
    } else if (s[j] === '"' || s[j] === "'") {
      const q = s[j];
      j++;
      while (j < s.length) {
        if (s[j] === '\\') j++;
        else if (s[j] === q) break;
        j++;
      }
    } else if (s[j] === '`') {
      j++;
      while (j < s.length) {
        if (s[j] === '\\') j++;
        else if (s[j] === '`') break;
        else if (s[j] === '$' && j+1 < s.length && s[j+1] === '{') {
          j += 2;
          let ed = 1;
          while (j < s.length && ed > 0) {
            if (s[j] === '{') ed++;
            else if (s[j] === '}') { ed--; if (ed === 0) break; }
            j++;
          }
        }
        j++;
      }
    }
    j++;
  }

  return s.slice(start, j);
}

// The outermost BalCard has shadow: "xl"
const vueSlot = extractSlotObj(vueWs, 'shadow: "xl", noBorder');
const verterSlot = extractSlotObj(verterWs, 'shadow: "xl", noBorder');

if (vueSlot) {
  console.log("=== Vue slot object (first 400 chars) ===");
  console.log(vueSlot.substring(0, 400));
  console.log("\n... (length:", vueSlot.length, ")");
} else {
  // Try opposite order
  const vueSlot2 = extractSlotObj(vueWs, 'noBorder: "", shadow: "xl"');
  if (vueSlot2) {
    console.log("=== Vue slot object (first 400 chars) ===");
    console.log(vueSlot2.substring(0, 400));
    console.log("\n... (length:", vueSlot2.length, ")");
  } else {
    console.log("Could not find Vue slot object");
  }
}

if (verterSlot) {
  console.log("\n=== Verter slot object (first 400 chars) ===");
  console.log(verterSlot.substring(0, 400));
  console.log("\n... (length:", verterSlot.length, ")");
} else {
  const verterSlot2 = extractSlotObj(verterWs, 'noBorder: "", shadow: "xl"');
  if (verterSlot2) {
    console.log("\n=== Verter slot object (first 400 chars) ===");
    console.log(verterSlot2.substring(0, 400));
    console.log("\n... (length:", verterSlot2.length, ")");
  } else {
    console.log("Could not find Verter slot object");
  }
}
