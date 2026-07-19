import { setDynamicProps as _setDynamicProps, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<p>Dynamic attribute", 1)
import { ref } from "vue"


export default {
  __name: 'dynamic-arg',
  __vapor: true,
  setup(__props) {

const attrName = ref("title")
const value = ref("Tooltip")


  const n0 = t0()
  _renderEffect(() => _setDynamicProps(n0, [{ [attrName.value]: value.value }]))
  return n0

}

}
