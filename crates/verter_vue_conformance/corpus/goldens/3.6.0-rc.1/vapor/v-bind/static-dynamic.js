import { setProp as _setProp, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<button type=button>Go", 1)
import { ref } from "vue"


export default {
  __name: 'static-dynamic',
  __vapor: true,
  setup(__props) {

const title = ref("Hello")
const disabled = ref(false)


  const n0 = t0()
  _renderEffect(() => {
    _setProp(n0, "title", title.value)
    _setProp(n0, "disabled", disabled.value)
  })
  return n0

}

}
