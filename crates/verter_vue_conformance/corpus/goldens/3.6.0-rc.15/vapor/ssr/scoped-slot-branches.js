import { defineVaporComponent as _defineVaporComponent } from "vue";
import ChildComp from "../components/child-comp.vue";
const _sfc_main = /* @__PURE__ */ _defineVaporComponent({
  __name: "scoped-slot-branches",
  setup(__props, { expose: __expose }) {
    __expose();
    const __returned__ = { ChildComp };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createIf as _createIf, extend as _extend, createComponent as _createComponent, template as _template } from 'vue';
const t0 = _template("<span>a", 2)
const t1 = _template(" ")

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n7 = _createComponent(_ctx.ChildComp, null, {
    "cell": _extend((_slotProps0) => {
      const n0 = _createIf(() => (_slotProps0.row.a), () => {
        const n2 = t0()
        return n2
      }, () => {
        const n4 = t1()
        _renderEffect(() => _setText(n4, _toDisplayString(_slotProps0.row.b)))
        return n4
      }, 393 /* TRUE_SINGLE_ROOT, FALSE_MULTI_ROOT, SLOT_ROOT, KEYED_INDEX_0 */)
      return n0
    }, { _: 1 /* NON_STABLE */ })
  }, true)
  return n7
}
_sfc_main.render = render
export default _sfc_main
