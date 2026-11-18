import { ref } from "vue"


const _sfc_main = {
  __name: 'teleport',
  setup(__props, { expose: __expose }) {
  __expose();

const open = ref(true)

const __returned__ = { open, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrRenderTeleport as _ssrRenderTeleport } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _ssrRenderTeleport(_push, (_push) => {
    _push(`<p class="overlay">Overlay</p>`)
  }, "body", !$setup.open, _parent)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
