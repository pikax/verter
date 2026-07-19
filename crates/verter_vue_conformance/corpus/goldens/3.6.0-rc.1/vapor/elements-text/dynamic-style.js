import { setStyle as _setStyle, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<div>Styled", 1)
import { ref } from "vue"


export default {
  __name: 'dynamic-style',
  __vapor: true,
  setup(__props) {

const color = ref("red")
const top = ref(10)


  const n0 = t0()
  _renderEffect(() => _setStyle(n0, { color: color.value, marginTop: top.value + 'px' }))
  return n0

}

}
