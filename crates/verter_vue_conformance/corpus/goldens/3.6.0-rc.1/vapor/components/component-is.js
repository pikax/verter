import { createDynamicComponent as _createDynamicComponent } from 'vue';
import { ref } from "vue"
import ChildComp from "./child-comp.vue"


export default {
  __name: 'component-is',
  __vapor: true,
  setup(__props) {

const current = ref(ChildComp)


  const n0 = _createDynamicComponent(() => (current.value), { label: "Dynamic" }, null, 1 /* SINGLE_ROOT */)
  return n0

}

}
