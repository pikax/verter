import { child as _child, applyCheckboxModel as _applyCheckboxModel, template as _template } from 'vue';
const t0 = _template("<label><input type=checkbox> Agree", 1)
import { ref } from "vue"


export default {
  __name: 'checkbox',
  __vapor: true,
  setup(__props) {

const agreed = ref(false)


  const n1 = t0()
  const n0 = _child(n1)
  _applyCheckboxModel(n0, () => (agreed.value), _value => (agreed.value = _value))
  return n1

}

}
