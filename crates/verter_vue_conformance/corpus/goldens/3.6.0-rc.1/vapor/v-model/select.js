import { applySelectModel as _applySelectModel, template as _template } from 'vue';
const t0 = _template("<select><option value=a>A</option><option value=b>B", 1)
import { ref } from "vue"


export default {
  __name: 'select',
  __vapor: true,
  setup(__props) {

const picked = ref("a")


  const n0 = t0()
  _applySelectModel(n0, () => (picked.value), _value => (picked.value = _value))
  return n0

}

}
