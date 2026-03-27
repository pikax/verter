import fs from "fs";

const d = JSON.parse(fs.readFileSync("C:/temp/ssr-full-vmodel-merge.json", "utf8"));
const mm = d.mismatches;

// Show a few element-plus casing examples with the actual diff
let shown = 0;
for (const m of mm) {
  if (shown >= 5) break;
  if (!m.file.includes("element-plus")) continue;

  const vueLines = (m.vue || "").split("\n");
  const verterLines = (m.verter || "").split("\n");
  const maxLen = Math.max(vueLines.length, verterLines.length);

  for (let i = 0; i < maxLen; i++) {
    const vl = (vueLines[i] || "").trim();
    const vrl = (verterLines[i] || "").trim();
    if (vl !== vrl) {
      let dp = 0;
      const ml = Math.min(vl.length, vrl.length);
      while (dp < ml && vl[dp] === vrl[dp]) dp++;
      if (dp < ml && vl[dp].toLowerCase() === vrl[dp].toLowerCase()) {
        console.log(`\n=== ${m.file} ===`);
        const start = Math.max(0, dp - 30);
        const end = Math.min(ml, dp + 40);
        console.log(`  Vue:    ...${vl.slice(start, end)}...`);
        console.log(`  Verter: ...${vrl.slice(start, end)}...`);
        // Extract the component name at the diff point
        const vName = vl.slice(dp).match(/^(\w+)/)?.[1] || "";
        const vrName = vrl.slice(dp).match(/^(\w+)/)?.[1] || "";
        console.log(`  Vue component: "${vl.slice(dp - 20, dp)}[${vName}]"`);
        console.log(`  Verter component: "${vrl.slice(dp - 20, dp)}[${vrName}]"`);
        shown++;
        break;
      }
    }
  }
}
