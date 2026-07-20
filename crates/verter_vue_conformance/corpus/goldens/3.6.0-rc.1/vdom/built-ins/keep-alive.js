import { resolveDynamicComponent as _resolveDynamicComponent, openBlock as _openBlock, createBlock as _createBlock, KeepAlive as _KeepAlive } from "vue"
import { ref } from "vue"
import ChildComp from "../components/child-comp.vue"


const _sfc_main = {
  __name: 'keep-alive',
  setup(__props, { expose: __expose }) {
  __expose();

const current = ref(ChildComp)

const __returned__ = { current, ref, ChildComp }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createBlock(_KeepAlive, { max: 2 }, [
    (_openBlock(), _createBlock(_resolveDynamicComponent($setup.current), { label: "Cached" }))
  ], 1024 /* DYNAMIC_SLOTS */))
}
_sfc_main.render = render
export default _sfc_main
