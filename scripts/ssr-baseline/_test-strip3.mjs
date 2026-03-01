import { extractSsrRenderBody, normalizeForComparison } from "./normalize.mjs";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, "../..");
const { parse, compileScript, compileTemplate } = require(
  require.resolve("@vue/compiler-sfc", { paths: [path.join(ROOT, "node_modules/.pnpm")] }),
);

// Look for balancer files in D:/dev/
const cacheDir = "D:/dev";
function* walkVue(dir, depth = 0) {
  if (depth > 6) return;
  try {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory() && !entry.name.startsWith('.') && entry.name !== 'node_modules') {
        yield* walkVue(full, depth + 1);
      } else if (entry.isFile() && entry.name.endsWith('.vue')) {
        yield full;
      }
    }
  } catch {}
}

for (const file of walkVue(cacheDir)) {
  const content = fs.readFileSync(file, 'utf-8');
  if (!content.includes('BalCard') || !content.includes('shadow="xl"') || !content.includes('noBorder')) continue;
  
  // Compile with Vue
  const { descriptor } = parse(content, { filename: file });
  if (!descriptor.template) continue;
  
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
  
  if (result.errors?.length) continue;
  
  const body = extractSsrRenderBody(result.code);
  if (!body) continue;
  
  const norm = normalizeForComparison(body);
  if (norm.includes('), _: 1, default:')) {
    console.log("Found problematic file:", file);
    // Show the raw around the issue
    const idx = body.indexOf(', _: 1');
    if (idx > 0) {
      console.log("\nRaw around _: 1:");
      console.log(body.substring(Math.max(0, idx-200), idx+50));
    }
    const nIdx = norm.indexOf('), _: 1, default:');
    if (nIdx > 0) {
      console.log("\nNormalized around issue:");
      console.log(norm.substring(Math.max(0, nIdx-100), nIdx+80));
    }
    break;
  }
}
