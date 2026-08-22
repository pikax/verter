import { resolveDynamicComponent as _resolveDynamicComponent, openBlock as _openBlock, createBlock as _createBlock, KeepAlive as _KeepAlive } from "vue"

import { ref } from "vue"
import ChildComp from "../components/child-comp.vue"


const _sfc_main = {
  __name: 'keep-alive',
  setup(__props) {

const current = ref(ChildComp)

return (_ctx, _cache) => {
  return (_openBlock(), _createBlock(_KeepAlive, { max: 2 }, [
    (_openBlock(), _createBlock(_resolveDynamicComponent(current.value), { label: "Cached" }))
  ], 1024 /* DYNAMIC_SLOTS */))
}
}

}
export default _sfc_main
