import { resolveDynamicComponent as _resolveDynamicComponent, openBlock as _openBlock, createBlock as _createBlock } from "vue"

import { ref } from "vue"
import ChildComp from "./child-comp.vue"


export default {
  __name: 'component-is',
  setup(__props) {

const current = ref(ChildComp)

return (_ctx, _cache) => {
  return (_openBlock(), _createBlock(_resolveDynamicComponent(current.value), { label: "Dynamic" }))
}
}

}
