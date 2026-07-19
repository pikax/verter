import { resolveDynamicComponent as _resolveDynamicComponent, openBlock as _openBlock, createBlock as _createBlock } from "vue"
import { ref } from "vue"
import ChildComp from "./child-comp.vue"


const _sfc_main = {
  __name: 'component-is',
  setup(__props, { expose: __expose }) {
  __expose();

const current = ref(ChildComp)

const __returned__ = { current, ref, ChildComp }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createBlock(_resolveDynamicComponent($setup.current), { label: "Dynamic" }))
}
_sfc_main.render = render
export default _sfc_main
