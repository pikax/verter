const path = require("path");
const fs = require("fs");
const { parse, compileScript, compileTemplate } = require("@vue/compiler-sfc");
const { VerterHost } = require(path.join(process.cwd(), "packages/native/index.js"));

// Pass the .vue file to trace as the first CLI arg, e.g.
//   node scripts/trace-norm.cjs path/to/component.vue
const filePath = process.argv[2];
if (!filePath) {
  console.error("trace-norm: pass a .vue file path as the first argument");
  process.exit(1);
}
const source = fs.readFileSync(filePath, "utf-8");
const filename = path.basename(filePath);

const { descriptor } = parse(source, { filename });
let bindingMetadata = {};
try {
  const scriptResult = compileScript(descriptor, { id: filename, inlineTemplate: false });
  bindingMetadata = scriptResult.bindings || {};
} catch {}
const vueResult = compileTemplate({
  source: descriptor.template.content,
  filename,
  id: filename,
  ssr: true,
  compilerOptions: { mode: "module", bindingMetadata },
});

const code = vueResult.code;
const fnIdx = code.indexOf("function ssrRender(");
const braceStart = code.indexOf("{", fnIdx);
let depth = 0,
  end = braceStart;
for (let i = braceStart; i < code.length; i++) {
  if (code[i] === "{") depth++;
  else if (code[i] === "}") {
    depth--;
    if (depth === 0) {
      end = i;
      break;
    }
  }
}
const body = code.slice(braceStart + 1, end).trim();

// Find the relevant mergeProps
const mpIdx = body.indexOf("_mergeProps({");
console.log("=== RAW Vue mergeProps ===");
console.log(body.slice(mpIdx, mpIdx + 600));
