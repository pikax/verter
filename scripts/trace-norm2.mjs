import { normalizeForComparison } from "./scripts/ssr-baseline/normalize.mjs";

// The raw Vue mergeProps section (whitespace collapsed, $setup → _ctx already done)
const raw = `_mergeProps({key: domain.key}, {ref_for: true}, index === 0 ? _ctx.formItemLayout : {}, {label: index === 0 ? 'Domains' : '', name: ['domains', index, 'value'], rules: {required: true, message: 'domain can not be null', trigger: 'change'}})`;

console.log("=== Input ===");
console.log(raw);
console.log("Length:", raw.length);

// Apply each major normalization step one at a time
let s = raw;

// Step: key stripping (lines 327-333 of normalize.mjs)
s = s.replace(/\{\s*key:\s*\d+\s*\}/g, "{}");
s = s.replace(/\{\s*key:\s*\d+,\s*/g, "{ ");
s = s.replace(/,\s*key:\s*\d+/g, "");
s = s.replace(/,\s*\{\s*key:\s*[^}]+\}/g, "");
s = s.replace(/\{\s*key:\s*[\w$.[\]]+,\s*/g, "{ ");
s = s.replace(/,\s*key:\s*[\w$.[\]]+/g, "");
console.log("\n--- After key strip ---");
console.log(s);
console.log("Has label:", s.includes("label"));

// Step: ref_for stripping
s = s.replace(/,\s*\{\s*ref_for:\s*true\s*\}/g, "");
console.log("\n--- After ref_for strip ---");
console.log(s);
console.log("Has label:", s.includes("label"));

// Step: empty obj strip
s = s.replace(/,\s*\{\s*\}/g, "");
console.log("\n--- After empty obj strip ---");
console.log(s);
console.log("Has label:", s.includes("label"));

// Step: sort mergeProps args (the tricky one)
// Let me skip this for now, check the remaining steps

// Step: strip single arg mergeProps
// Count args by looking for commas at depth 1
console.log("\n--- Before stripSingleArgMergeProps ---");
console.log(s);
