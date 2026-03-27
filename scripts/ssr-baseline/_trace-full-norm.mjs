import { normalizeForComparison } from "./normalize.mjs";

const input = `_push(\`<input\${_ssrRenderAttrs(_mergeProps({ ..._ctx.$attrs, onChange: $setup.updateValue }, { id: $setup.uuid, checked: $props.modelValue, class: "input", type: "checkbox" }))}\`)`;

// Find which part of normalization removes id:
// Run normalization and check intermediate results by progressively
// building the normalized string

const output = normalizeForComparison(input, new Map());
console.log("Input has id:", input.includes("id:"));
console.log("Output has id:", output.includes("id:"));
console.log("\nFinal output:");
console.log(output);

// Let me also check if it's the mergeProps object merging
console.log("\nLooking for _mergeProps in output:", output.includes("_mergeProps"));
