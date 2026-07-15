const { compileTemplate, parse, compileScript } = require("@vue/compiler-sfc");
const fs = require("fs");
const path = require("path");
// Usage: node check-codegen-story.cjs <mismatches.json> <corpus-root>
//   (corpus-root may also come from VERTER_COMPARE_ROOT)
const inputPath = process.argv[2];
const corpusRoot = process.argv[3] ?? process.env.VERTER_COMPARE_ROOT;
if (!inputPath || !corpusRoot) {
  console.error("usage: node check-codegen-story.cjs <mismatches.json> <corpus-root>");
  process.exit(1);
}
const d = JSON.parse(fs.readFileSync(inputPath, "utf8"));

// Find CodeGen.story.vue
for (const r of d.mismatches) {
  if (!r.file.includes("CodeGen.story.vue")) continue;

  const fullPath = path.join(corpusRoot, r.file);
  const source = fs.readFileSync(fullPath, "utf8");
  const { descriptor } = parse(source);

  let bindingMetadata = {};
  if (descriptor.script || descriptor.scriptSetup) {
    try {
      const scriptResult = compileScript(descriptor, {
        id: fullPath,
        inlineTemplate: false,
      });
      bindingMetadata = scriptResult.bindings || {};
    } catch (e) {
      console.log("Script error:", e.message.slice(0, 100));
    }
  }

  const { code } = compileTemplate({
    source: descriptor.template.content,
    filename: fullPath,
    id: "test",
    ssr: true,
    compilerOptions: { bindingMetadata },
  });

  // Find DYNAMIC slot flags in the VDOM fallback
  const elseIdx = code.indexOf("} else {");
  if (elseIdx !== -1) {
    const vdom = code.slice(elseIdx);
    const flags = [...vdom.matchAll(/_: (\d+) \/\* ([A-Z_]+) \*\//g)];
    console.log("VDOM fallback slot flags:", flags.map((m) => `${m[1]}/${m[2]}`).join(", "));

    // Show context around first DYNAMIC flag in VDOM
    const dynIdx = vdom.indexOf("_: 2 /* DYNAMIC */");
    if (dynIdx !== -1) {
      const ctx = vdom.slice(Math.max(0, dynIdx - 200), dynIdx + 50);
      console.log("\nContext around DYNAMIC in VDOM fallback:");
      console.log(ctx);
    }
  }
  break;
}
