import { VaporTeleport as _VaporTeleport, createComponent as _createComponent, template as _template } from 'vue';
const t0 = _template("<p class=overlay>Overlay", 2)
import { ref } from "vue"


export default {
  __name: 'teleport',
  __vapor: true,
  setup(__props) {

const open = ref(true)


  const n1 = _createComponent(_VaporTeleport, {
    to: "body",
    disabled: () => (!open.value)
  }, () => {
    const n0 = t0()
    return n0
  }, true)
  return n1

}

}
