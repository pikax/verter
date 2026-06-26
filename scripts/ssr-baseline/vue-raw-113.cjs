const { compileTemplate, parse } = require("@vue/compiler-sfc");
const fs = require("fs");
// Usage: node vue-raw-113.cjs <mismatches.json> <corpus-root>
//   (corpus-root may also come from VERTER_COMPARE_ROOT)
const inputPath = process.argv[2];
const corpusRoot = process.argv[3] ?? process.env.VERTER_COMPARE_ROOT;
if (!inputPath || !corpusRoot) {
  console.error("usage: node vue-raw-113.cjs <mismatches.json> <corpus-root>");
  process.exit(1);
}
const d = JSON.parse(fs.readFileSync(inputPath, "utf8"));
for (const r of d.mismatches) {
  const v = r.vue || "",
    t = r.verter || "";
  if (v.replace(/\s+/g, "") !== t.replace(/\s+/g, "")) continue;
  if (!v.includes(") ,")) continue;

  const fullPath = require("path").join(corpusRoot, r.file);
  const source = fs.readFileSync(fullPath, "utf8");
  const { descriptor } = parse(source);
  if (!descriptor.template) continue;
  const { code } = compileTemplate({
    source: descriptor.template.content,
    filename: r.file,
    id: "test",
    ssr: true,
  });
  const idx = code.indexOf(") ,");
  if (idx !== -1) {
    console.log("File:", r.file.split("/").slice(-2).join("/"));
    console.log('Raw around ") ,":');
    console.log(code.slice(Math.max(0, idx - 80), idx + 80));
  } else {
    console.log("File:", r.file.split("/").slice(-2).join("/"), '- NO raw ") ," pattern');
    // find props area
    const pi = code.indexOf("_ssrRenderComponent");
    if (pi !== -1) {
      console.log("Raw _ssrRenderComponent call:");
      console.log(code.slice(pi, pi + 800));
    }
  }
  break;
}
