import { openBlock as _openBlock, createBlock as _createBlock } from "vue"

import ChildComp from "./child-comp.vue"


const _sfc_main = {
  __name: 'parent-props-events',
  setup(__props) {

function onSelect() {}

return (_ctx, _cache) => {
  return (_openBlock(), _createBlock(ChildComp, {
    label: "Pick",
    count: 3,
    onSelect: onSelect
  }))
}
}

}
export default _sfc_main
