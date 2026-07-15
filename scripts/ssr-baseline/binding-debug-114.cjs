const path = require("path");
const fs = require("fs");
const { parse, compileScript, compileTemplate } = require("@vue/compiler-sfc");
// Usage: node binding-debug-114.cjs <mismatches.json> <corpus-root>
//   (corpus-root may also come from VERTER_COMPARE_ROOT)
const inputPath = process.argv[2];
const corpusRoot = process.argv[3] ?? process.env.VERTER_COMPARE_ROOT;
if (!inputPath || !corpusRoot) {
  console.error("usage: node binding-debug-114.cjs <mismatches.json> <corpus-root>");
  process.exit(1);
}
const d = JSON.parse(fs.readFileSync(inputPath, "utf8"));

const bp = (s) =>
  s
    .replace(/\$setup\./g, "_ctx.")
    .replace(/\$setup\["/g, '_ctx["')
    .replace(/\$props\./g, "_ctx.")
    .replace(/\$props\["/g, '_ctx["');

let count = 0;
for (const r of d.mismatches) {
  const v = r.vue || "",
    t = r.verter || "";
  if (bp(v) !== bp(t)) continue;

  const fullPath = path.join(corpusRoot, r.file);
  let source;
  try {
    source = fs.readFileSync(fullPath, "utf8");
  } catch {
    continue;
  }

  const { descriptor } = parse(source);
  let bindingMetadata = {};
  let scriptError = null;

  if (descriptor.script || descriptor.scriptSetup) {
    try {
      const scriptResult = compileScript(descriptor, {
        id: fullPath,
        inlineTemplate: false,
      });
      bindingMetadata = scriptResult.bindings || {};
    } catch (e) {
      scriptError = e.message;
    }
  }

  const bindings = Object.entries(bindingMetadata)
    .filter(([k, v]) =>
      [
        "setup-const",
        "setup-let",
        "setup-maybe-ref",
        "setup-reactive-const",
        "setup-ref",
        "props",
        "props-aliased",
      ].includes(v),
    )
    .slice(0, 8);

  count++;
  console.log(
    `${r.file.split("/").slice(-2).join("/")}: ${scriptError ? "SCRIPT_ERROR: " + scriptError.slice(0, 80) : "bindings=" + JSON.stringify(Object.fromEntries(bindings)).slice(0, 120)}`,
  );

  if (count >= 22) break;
}
