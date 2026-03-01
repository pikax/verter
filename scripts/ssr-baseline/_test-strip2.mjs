import { normalizeForComparison, extractSsrRenderBody } from "./normalize.mjs";

// Vue's output where default slot VDOM fallback contains nested components
const vueSsr = `function ssrRender(_ctx, _push, _parent, _attrs) {
  _push(_ssrRenderComponent(_ctx["BalCard"], {noBorder: "", shadow: "xl"}, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(_ssrRenderComponent(_ctx["BalStack"], {spacing: "sm", vertical: ""}, {
          default: _withCtx((_, _push, _parent, _scopeId) => {
            if (_push) {
              _push(\`<span\${_scopeId}>hello</span>\`)
            } else {
              return [
                _createVNode("span", null, "hello")
              ]
            }
          }),
          _: 1 /* STABLE */
        }, _parent, _scopeId))
      } else {
        return [
          _createVNode(_ctx["BalStack"], {spacing: "sm", vertical: ""}, {
            default: _withCtx((_, _push, _parent) => [_createVNode("span", null, "hello")]),
            _: 1 /* STABLE */
          })
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}`;

const body = extractSsrRenderBody(vueSsr);
console.log("Body:");
console.log(body);
console.log("\nNormalized:");
console.log(normalizeForComparison(body));
