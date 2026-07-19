import { createDynamicComponent as _createDynamicComponent } from 'vue';
import { ref } from "vue"
import ChildComp from "./child-comp.vue"


const _sfc_main = {
  __name: 'component-is',
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
  const n0 = _createDynamicComponent(() => (_ctx.current), { label: "Dynamic" }, null, 1 /* SINGLE_ROOT */)
  return n0
}
_sfc_main.render = render
export default _sfc_main
