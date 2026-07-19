import { setHtml as _setHtml, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<div>", 1)
import { ref } from "vue"


export default {
  __name: 'html',
  __vapor: true,
  setup(__props) {

const raw = ref("<b>bold</b>")


  const n0 = t0()
  _renderEffect(() => _setHtml(n0, raw.value))
  return n0

}

}
