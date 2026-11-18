import { defineVaporComponent as _defineVaporComponent } from "vue";
import { ref } from "vue";
const _sfc_main = /* @__PURE__ */ _defineVaporComponent({
  __name: "scoped-style-root",
  setup(__props, { expose: __expose }) {
    __expose();
    const n = ref(1);
    const __returned__ = { n };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<p class=a> ", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  const x0 = _txt(n0)
  _renderEffect(() => _setText(x0, _toDisplayString(_ctx.n)))
  return n0
}
_sfc_main.render = render
export default _sfc_main
