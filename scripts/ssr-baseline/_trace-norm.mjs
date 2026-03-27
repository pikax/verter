import { extractSsrRenderBody, extractImports } from "./normalize.mjs";
import fs from "fs";

// Read the normalize.mjs source and manually trace what happens
// to "id: $setup.uuid"
const normalizeSource = fs.readFileSync("./scripts/ssr-baseline/normalize.mjs", "utf-8");

// Let's manually trace with a simpler input
let s = `_mergeProps({..._ctx.$attrs, onChange: $setup.updateValue}, {id: $setup.uuid, checked: $props.modelValue, class: "input", type: "checkbox"})`;

// Step by step normalization (manual trace of key rules)
console.log("=== Step 0 (input):");
console.log(s);

// Binding normalization: $setup.xxx → _ctx["xxx"], $props.xxx → _ctx["xxx"]
s = s.replace(/\$setup\.(\w+)/g, '_ctx["$1"]');
s = s.replace(/\$props\.(\w+)/g, '_ctx["$1"]');
console.log("\n=== Step 1 (binding normalize):");
console.log(s);

// Event stripping: onChange: ...
s = s.replace(/,?\s*\bon\w+:\s*[^,}]+/g, "");
console.log("\n=== Step 2 (event strip):");
console.log(s);

// id stripping rules from lines 403-405
const before = s;
s = s.replace(/,\s*\{\s*id:\s*_ctx\.\w+\s*\}/g, "");
s = s.replace(/,\s*id:\s*_ctx\.\w+/g, "");
s = s.replace(/\{\s*id:\s*_ctx\.\w+,\s*/g, "{");
console.log("\n=== Step 3 (id strip _ctx.xxx):");
console.log(s);
console.log("Changed:", s !== before);

// Check if id: _ctx["xxx"] is caught by a different rule
// Bracket notation: _ctx["uuid"]
const hasIdBracket = s.includes('id: _ctx["uuid"]') || s.includes('id: _ctx["uuid"]');
console.log('Has id: _ctx["uuid"]:', hasIdBracket);

// id strip from loop (line 662)
const before2 = s;
s = s.replace(/,\s*id:\s*_ctx\.\w+/g, "");
console.log("\n=== Step 4 (id strip loop):");
console.log(s);
console.log("Changed:", s !== before2);

// Check: is id: _ctx["uuid"] pattern caught elsewhere?
// Let me check all id-related patterns
console.log('\nFinal has "id:":', s.includes("id:"));
