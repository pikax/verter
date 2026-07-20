import { VaporKeepAlive as _VaporKeepAlive, createDynamicComponent as _createDynamicComponent, extend as _extend, createComponent as _createComponent } from 'vue';
import { ref } from "vue"
import ChildComp from "../components/child-comp.vue"


const _sfc_main = {
  __name: 'keep-alive',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const current = ref(ChildComp)

const __returned__ = { current, ref, ChildComp }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, $props, $emit, $attrs, $slots) {
  const n1 = _createComponent(_VaporKeepAlive, { max: 2 }, _extend(() => {
    const n0 = _createDynamicComponent(() => (_ctx.current), { label: "Cached" }, null, 4 /* SLOT_ROOT */)
    return n0
  }, { _: 8 /* NON_STABLE */ }), true)
  return n1
}
_sfc_main.render = render
export default _sfc_main
