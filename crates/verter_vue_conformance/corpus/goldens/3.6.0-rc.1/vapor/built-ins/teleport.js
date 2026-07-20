import { VaporTeleport as _VaporTeleport, createComponent as _createComponent, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'teleport',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const open = ref(true)

const __returned__ = { open, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<p class=overlay>Overlay", 2)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n1 = _createComponent(_VaporTeleport, {
    to: "body",
    disabled: () => (!_ctx.open)
  }, () => {
    const n0 = t0()
    return n0
  }, true)
  return n1
}
_sfc_main.render = render
export default _sfc_main
