import { openBlock as _openBlock, createBlock as _createBlock } from "vue"
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
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createBlock($setup["ChildComp"], {
    label: "Pick",
    count: 3,
    onSelect: $setup.onSelect
  }))
}
_sfc_main.render = render
export default _sfc_main
