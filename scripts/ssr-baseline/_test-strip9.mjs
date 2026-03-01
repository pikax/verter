// Minimal reproduction of the sortSlotProperties + stripVdomFallback interaction
import { normalizeForComparison } from "./normalize.mjs";

// Simulate what Vue produces for a single-slot component with VDOM fallback
// The key pattern is:
// { default: _withCtx((_, _push, _parent) => { if (_push) { SSR_CODE } else { return [VDOM_CODE] } }), _: 1 }

const vueRaw = `{ default: _withCtx((_, _push, _parent, _scopeId) => { if (_push) { _push(\`<div>\${_ssrInterpolate(_ctx.msg)}</div>\`) } else { return [_createVNode("div", null, _toDisplayString(_ctx.msg), 1)] } }), _: 1 /* STABLE */ }`;

const verterRaw = `{ default: _withCtx((_, _push, _parent, _scopeId) => { if (_push) { _push(\`<div>\${_ssrInterpolate(_ctx.msg)}</div>\`) } }), _: 1 }`;

console.log("Vue normalized:");
const vueNorm = normalizeForComparison(vueRaw);
console.log(vueNorm);

console.log("\nVerter normalized:");
const verterNorm = normalizeForComparison(verterRaw);
console.log(verterNorm);

console.log("\nMatch:", vueNorm === verterNorm);

// Now test with a more complex case: component inside the slot
const vueComplex = `_push(_ssrRenderComponent(_component_Card, { title: "hello" }, { default: _withCtx((_, _push, _parent, _scopeId) => { if (_push) { _push(\`<span\${ _scopeId }>\${ _ssrInterpolate(_ctx.msg) }</span>\`) } else { return [ _createVNode("span", null, _toDisplayString(_ctx.msg), 1) ] } }), _: 1 /* STABLE */ }, _parent))`;

const verterComplex = `_push(_ssrRenderComponent(_component_Card, { title: "hello" }, { default: _withCtx((_, _push, _parent, _scopeId) => { if (_push) { _push(\`<span\${ _scopeId }>\${ _ssrInterpolate(_ctx.msg) }</span>\`) } }), _: 1 }, _parent))`;

console.log("\n=== Complex case ===");
const vueComplexNorm = normalizeForComparison(vueComplex);
const verterComplexNorm = normalizeForComparison(verterComplex);

console.log("Vue:");
console.log(vueComplexNorm);
console.log("\nVerter:");
console.log(verterComplexNorm);
console.log("\nMatch:", vueComplexNorm === verterComplexNorm);

// Now with nested components that have their own slots
const vueNested = `_push(_ssrRenderComponent(_component_Outer, { shadow: "xl", noBorder: "" }, { default: _withCtx((_, _push, _parent, _scopeId) => { if (_push) { _push(_ssrRenderComponent(_component_Inner, { spacing: "sm" }, { default: _withCtx((_, _push, _parent, _scopeId) => { if (_push) { _push(\`<span\${ _scopeId }>hello</span>\`) } else { return [ _createVNode("span", null, "hello") ] } }), _: 1 /* STABLE */ }, _parent, _scopeId)) } else { return [ _createVNode(_component_Inner, { spacing: "sm" }, { default: _withCtx(() => [ _createVNode("span", null, "hello") ]), _: 1 /* STABLE */ }) ] } }), _: 1 /* STABLE */ }, _parent))`;

const verterNested = `_push(_ssrRenderComponent(_component_Outer, { shadow: "xl", noBorder: "" }, { default: _withCtx((_, _push, _parent, _scopeId) => { if (_push) { _push(_ssrRenderComponent(_component_Inner, { spacing: "sm" }, { default: _withCtx((_, _push, _parent, _scopeId) => { if (_push) { _push(\`<span\${ _scopeId }>hello</span>\`) } }), _: 1 }, _parent, _scopeId)) } }), _: 1 }, _parent))`;

console.log("\n=== Nested case ===");
const vueNestedNorm = normalizeForComparison(vueNested);
const verterNestedNorm = normalizeForComparison(verterNested);

console.log("Vue:");
console.log(vueNestedNorm);
console.log("\nVerter:");
console.log(verterNestedNorm);
console.log("\nMatch:", vueNestedNorm === verterNestedNorm);

if (vueNestedNorm !== verterNestedNorm) {
  // Find first diff
  for (let i = 0; i < Math.max(vueNestedNorm.length, verterNestedNorm.length); i++) {
    if (vueNestedNorm[i] !== verterNestedNorm[i]) {
      console.log("\nFirst diff at index", i);
      console.log("Vue:    ..." + vueNestedNorm.substring(Math.max(0, i-30), i+30));
      console.log("Verter: ..." + verterNestedNorm.substring(Math.max(0, i-30), i+30));
      break;
    }
  }
}
