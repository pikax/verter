import { applyTextModel as _applyTextModel, template as _template } from 'vue';
const t0 = _template("<input placeholder=\"Type here\">", 1)
import { ref } from "vue"


export default {
  __name: 'input',
  __vapor: true,
  setup(__props) {

const text = ref("")


  const n0 = t0()
  _applyTextModel(n0, () => (text.value), _value => (text.value = _value))
  return n0

}

}
