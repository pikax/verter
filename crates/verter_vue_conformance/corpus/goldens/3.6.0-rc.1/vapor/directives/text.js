import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<p> ", 1)
import { ref } from "vue"


export default {
  __name: 'text',
  __vapor: true,
  setup(__props) {

const plain = ref("plain text")


  const n0 = t0()
  const x0 = _txt(n0)
  _renderEffect(() => _setText(x0, _toDisplayString(plain.value)))
  return n0

}

}
