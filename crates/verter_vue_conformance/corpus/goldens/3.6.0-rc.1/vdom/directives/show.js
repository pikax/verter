import { vShow as _vShow, withDirectives as _withDirectives, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'show',
  setup(__props, { expose: __expose }) {
  __expose();

const visible = ref(true)

const __returned__ = { visible, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return _withDirectives((_openBlock(), _createElementBlock("p", null, "Peekaboo", 512 /* NEED_PATCH */)), [
    [_vShow, $setup.visible]
  ])
}
_sfc_main.render = render
export default _sfc_main
