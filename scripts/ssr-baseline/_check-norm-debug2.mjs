import { extractSsrRenderBody, normalizeForComparison, extractImports } from "./normalize.mjs";

// Simpler test: does normalization strip id: from mergeProps?
const test1 = `_mergeProps({ ..._ctx.$attrs, onChange: $setup.updateValue }, { id: $setup.uuid, checked: $props.modelValue, type: "checkbox" })`;
const normalized1 = normalizeForComparison(test1, new Map());
console.log("Input:", test1);
console.log("Normalized:", normalized1);
console.log("Has id:", normalized1.includes("id:"));
console.log();

// Test with just id in object
const test2 = `{id: $setup.uuid, checked: $props.modelValue}`;
const normalized2 = normalizeForComparison(test2, new Map());
console.log("Input:", test2);
console.log("Normalized:", normalized2);
console.log("Has id:", normalized2.includes("id:"));
