import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, "../..");
const { parse, compileScript, compileTemplate } = require(
  require.resolve("@vue/compiler-sfc", { paths: [path.join(ROOT, "node_modules/.pnpm")] }),
);

// Find the file numbered "1" in the comparison — need to figure out which file that is
// from the JSON — but the keys are just numbers. Let me search for a balancer project file
// that uses BalCard
const testDir = process.env.VERTER_TEST_REPOS;
if (!testDir) { console.error('Set VERTER_TEST_REPOS env var'); process.exit(1); }
const testFiles = [];

function walk(dir, depth = 0) {
  if (depth > 5) return;
  try {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory() && !entry.name.startsWith('.') && entry.name !== 'node_modules') {
        walk(full, depth + 1);
      } else if (entry.isFile() && entry.name.endsWith('.vue')) {
        const content = fs.readFileSync(full, 'utf-8');
        if (content.includes('BalCard') && content.includes('shadow="xl"') && content.includes('noBorder')) {
          testFiles.push(full);
        }
      }
    }
  } catch {}
}
walk(path.join(testDir, "balancer-frontend"));

if (testFiles.length > 0) {
  const file = testFiles[0];
  console.log("File:", file);
  const source = fs.readFileSync(file, 'utf-8');
  const { descriptor } = parse(source, { filename: file });
  
  let bindingMetadata = {};
  if (descriptor.script || descriptor.scriptSetup) {
    try {
      const sr = compileScript(descriptor, { id: file, inlineTemplate: false });
      bindingMetadata = sr.bindings || {};
    } catch {}
  }
  
  const result = compileTemplate({
    source: descriptor.template.content,
    filename: file,
    id: file,
    ssr: true,
    compilerOptions: { mode: "module", bindingMetadata },
  });
  
  // Show the slot object portion
  const idx = result.code.indexOf('BalCard');
  if (idx > 0) {
    console.log("\nVue SSR output around BalCard (500 chars):");
    console.log(result.code.substring(idx - 50, idx + 450));
  }
} else {
  console.log("No matching file found");
}
