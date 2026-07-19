import { setClass as _setClass, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<div>Classy", 1)
import { ref } from "vue"


export default {
  __name: 'dynamic-class',
  __vapor: true,
  setup(__props) {

const active = ref(true)
const size = ref("lg")


  const n0 = t0()
  _renderEffect(() => _setClass(n0, ['card', { active: active.value }, 'size-' + size.value]))
  return n0

}

}
