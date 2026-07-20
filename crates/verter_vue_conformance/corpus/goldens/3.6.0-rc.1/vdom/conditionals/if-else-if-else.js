import { openBlock as _openBlock, createElementBlock as _createElementBlock, createCommentVNode as _createCommentVNode, toDisplayString as _toDisplayString } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'if-else-if-else',
  setup(__props, { expose: __expose }) {
  __expose();

const status = ref("loading")

const __returned__ = { status, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const _hoisted_1 = { key: 0 }
const _hoisted_2 = { key: 1 }
const _hoisted_3 = { key: 2 }

function render(_ctx, _cache, $props, $setup, $data, $options) {
  return ($setup.status === 'loading')
    ? (_openBlock(), _createElementBlock("p", _hoisted_1, "Loading"))
    : ($setup.status === 'error')
      ? (_openBlock(), _createElementBlock("p", _hoisted_2, "Failed"))
      : (_openBlock(), _createElementBlock("p", _hoisted_3, "Done: " + _toDisplayString($setup.status), 1 /* TEXT */))
}
_sfc_main.render = render
export default _sfc_main
