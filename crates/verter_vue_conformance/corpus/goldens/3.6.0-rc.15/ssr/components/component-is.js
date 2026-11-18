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
import { resolveDynamicComponent as _resolveDynamicComponent, mergeProps as _mergeProps, createVNode as _createVNode } from "vue"
import { ssrRenderVNode as _ssrRenderVNode } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _ssrRenderVNode(_push, _createVNode(_resolveDynamicComponent($setup.current), _mergeProps({ label: "Dynamic" }, _attrs), null), _parent)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
