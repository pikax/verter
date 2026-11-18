import { defineVaporComponent as _defineVaporComponent } from "vue";
import ChildComp from "../components/child-comp.vue";
const show = true;
const _sfc_main = /* @__PURE__ */ _defineVaporComponent({
  __name: "conditional-slot",
  setup(__props, { expose: __expose }) {
    __expose();
    const __returned__ = { show, ChildComp };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { createComponent as _createComponent, template as _template } from 'vue';
const t0 = _template("x", 2)

function render(_ctx, $props, $emit, $attrs, $slots) {
  let s
  const n2 = _createComponent(_ctx.ChildComp, null, {
    $: [
      () => (_ctx.show
        ? {
          name: "actions",
          fn: s || (s = () => {
            const n0 = t0()
            return n0
          })
        }
        : void 0)
    ]
  }, true)
  return n2
}
_sfc_main.render = render
export default _sfc_main
