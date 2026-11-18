import ChildComp from "./child-comp.vue"


const _sfc_main = {
  __name: 'parent-props-events',
  setup(__props, { expose: __expose }) {
  __expose();

function onSelect() {}

const __returned__ = { onSelect, ChildComp }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(_ssrRenderComponent($setup["ChildComp"], _mergeProps({
    label: "Pick",
    count: 3,
    onSelect: $setup.onSelect
  }, _attrs), null, _parent))
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
