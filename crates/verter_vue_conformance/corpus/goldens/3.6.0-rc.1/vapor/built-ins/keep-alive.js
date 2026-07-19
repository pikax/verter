import { VaporKeepAlive as _VaporKeepAlive, createDynamicComponent as _createDynamicComponent, extend as _extend, createComponent as _createComponent } from 'vue';
import { ref } from "vue"
import ChildComp from "../components/child-comp.vue"


export default {
  __name: 'keep-alive',
  __vapor: true,
  setup(__props) {

const current = ref(ChildComp)


  const n1 = _createComponent(_VaporKeepAlive, { max: 2 }, _extend(() => {
    const n0 = _createDynamicComponent(() => (current.value), { label: "Cached" }, null, 4 /* SLOT_ROOT */)
    return n0
  }, { _: 8 /* NON_STABLE */ }), true)
  return n1

}

}
