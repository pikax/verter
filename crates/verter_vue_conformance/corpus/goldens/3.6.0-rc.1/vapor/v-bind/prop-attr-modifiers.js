import { setDOMProp as _setDOMProp, setAttr as _setAttr, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<div>", 1)
import { ref } from "vue"


export default {
  __name: 'prop-attr-modifiers',
  __vapor: true,
  setup(__props) {

const text = ref("inner")
const dataX = ref("x")


  const n0 = t0()
  _renderEffect(() => {
    _setDOMProp(n0, "text-content", text.value)
    _setAttr(n0, "data-x", dataX.value)
  })
  return n0

}

}
