import { setProp as _setProp, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<div>", 1)
import { ref } from "vue"


export default {
  __name: 'same-name-shorthand',
  __vapor: true,
  setup(__props) {

const id = ref("a1")
const title = ref("Hi")


  const n0 = t0()
  _renderEffect(() => {
    _setProp(n0, "id", id.value)
    _setProp(n0, "title", title.value)
  })
  return n0

}

}
