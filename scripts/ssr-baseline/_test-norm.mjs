import { normalizeForComparison } from "./normalize.mjs";

const vue =
  '_push(_ssrRenderComponent(comp, {}, {default: _withCtx((_, _push, _parent, _scopeId) => {if (_push) {_push(`<div>hello</div>`)} else {return [_createVNode("div", null, "hello")]}}), _: 1}, _parent))';

const verter =
  "_push(_ssrRenderComponent(comp, {}, {default: _withCtx((_, _push, _parent) => {if (_push) {_push(`<div>hello</div>`)}}), _: 1}, _parent))";

console.log("Vue normalized:");
console.log(normalizeForComparison(vue));
console.log("\nVerter normalized:");
console.log(normalizeForComparison(verter));
console.log("\nMatch:", normalizeForComparison(vue) === normalizeForComparison(verter));
