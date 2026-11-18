import { defineVaporComponent as _defineVaporComponent } from "vue";
import { ref } from "vue";
import ChildComp from "../components/child-comp.vue";
const _sfc_main = /* @__PURE__ */ _defineVaporComponent({
  __name: "slot-handler-statements",
  setup(__props, { expose: __expose }) {
    __expose();
    const a = ref("");
    const b = ref("");
    const __returned__ = { a, b, ChildComp };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { on as _on, createComponent as _createComponent, template as _template } from 'vue';
const t0 = _template("<button>reset")

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n1 = _createComponent(_ctx.ChildComp, null, () => {
    const n0 = t0()
    _on(n0, "click", () => {_ctx.a = ''; _ctx.b = ''})
    return n0
  }, true)
  return n1
}
_sfc_main.render = render
export default _sfc_main
