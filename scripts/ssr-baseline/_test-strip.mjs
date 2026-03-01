import { normalizeForComparison, extractSsrRenderBody } from "./normalize.mjs";

// Simulate Vue SSR output with _scopeId in slot
const vueSsr = `function ssrRender(_ctx, _push, _parent, _attrs) {
  _push(_ssrRenderComponent(_ctx["BalCard"], {noBorder: "", shadow: "xl"}, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(\`<div\${_scopeId}>hello</div>\`)
      } else {
        return [
          _createVNode("div", null, "hello")
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}`;

const body = extractSsrRenderBody(vueSsr);
console.log("Extracted body:");
console.log(body);
console.log("\nNormalized:");
console.log(normalizeForComparison(body));
