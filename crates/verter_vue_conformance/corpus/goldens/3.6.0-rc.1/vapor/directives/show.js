import { applyVShow as _applyVShow, template as _template } from 'vue';
const t0 = _template("<p>Peekaboo", 1)
import { ref } from "vue"


export default {
  __name: 'show',
  __vapor: true,
  setup(__props) {

const visible = ref(true)


  const n0 = t0()
  _applyVShow(n0, () => (visible.value))
  return n0

}

}
