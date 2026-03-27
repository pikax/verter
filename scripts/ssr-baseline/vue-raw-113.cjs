const { compileTemplate, parse } = require("@vue/compiler-sfc");
const fs = require("fs");
const d = JSON.parse(fs.readFileSync("C:/temp/ssr-iter-113.json", "utf8"));
for (const r of d.mismatches) {
  const v = r.vue || "",
    t = r.verter || "";
  if (v.replace(/\s+/g, "") !== t.replace(/\s+/g, "")) continue;
  if (!v.includes(") ,")) continue;

  const fullPath = "D:/dev/" + r.file;
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
