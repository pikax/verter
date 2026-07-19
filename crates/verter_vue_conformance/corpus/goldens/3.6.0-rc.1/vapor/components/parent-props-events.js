import { createComponent as _createComponent } from 'vue';
import ChildComp from "./child-comp.vue"


export default {
  __name: 'parent-props-events',
  __vapor: true,
  setup(__props) {

function onSelect() {}


  const n0 = _createComponent(ChildComp, {
    label: "Pick",
    count: 3,
    onSelect: () => onSelect
  }, null, true)
  return n0

}

}
