import { normalizeForComparison } from "./normalize.mjs";

const body = `_push(_ssrRenderComponent(_component_a_form_item, _mergeProps({key: domain.key}, {ref_for: true}, index === 0 ? _ctx.formItemLayout : {}, {label: index === 0 ? 'Domains' : '', name: ['domains', index, 'value'], rules: {required: true, message: 'domain can not be null', trigger: 'change'}}), {default: _withCtx((_, _push, _parent) => {if (_push) {_push('hello')}}), _: 1}, _parent))`;

// Apply everything up to line 451 (pre-merge) by running full normalizeForComparison
// but let me instrument it. Actually let me just test each of the 3 suspect functions individually

// State after previous steps:
let s = `_push(_ssrRenderComponent(_component_a_form_item, _mergeProps({key: domain.key}, index === 0 ? _ctx.formItemLayout : {}, {label: index === 0 ? 'Domains' : '', name: ['domains', index, 'value'], rules: {required: true, message: 'domain can not be null', trigger: 'change'}}), {default: _withCtx((_, _push, _parent) => {if (_push) {_push('hello')}}), _: 1}, _parent))`;

console.log("Start has label:", s.includes("label"));

// Full normalize to see the effect
const result = normalizeForComparison(s);
console.log("Full normalize has label:", result.includes("label"));
console.log("Full normalize:", result);

// Now let me also test: what if I disable mergeMergePropsObjects only?
// Let me test by running normalizeForComparison on a version that
// already has the objects merged (simulating what merge does)
const premerged = `_push(_ssrRenderComponent(_component_a_form_item, _mergeProps({key: domain.key}, index === 0 ? _ctx.formItemLayout : {}, {label: index === 0 ? 'Domains' : '', name: ['domains', index, 'value'], rules: {required: true, message: 'domain can not be null', trigger: 'change'}}), {default: _withCtx((_, _push, _parent) => {if (_push) {_push('hello')}}), _: 1}, _parent))`;

const r2 = normalizeForComparison(premerged);
console.log("\nPremerged result:", r2);
console.log("Has label:", r2.includes("label"));
