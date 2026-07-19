import { createComponent as _createComponent } from 'vue';
import ChildComp from "./child-comp.vue"


const _sfc_main = {
  __name: 'parent-props-events',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

function onSelect() {}

const __returned__ = { onSelect, ChildComp }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = _createComponent(_ctx.ChildComp, {
    label: "Pick",
    count: 3,
    onSelect: () => _ctx.onSelect
  }, null, true)
  return n0
}
_sfc_main.render = render
export default _sfc_main
