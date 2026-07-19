import { withKeys as _withKeys, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _sfc_main = {
  __name: 'key-modifiers',
  setup(__props, { expose: __expose }) {
  __expose();

function onEnter() {}
function onEsc() {}

const __returned__ = { onEnter, onEsc }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("input", {
    onKeyup: _withKeys($setup.onEnter, ["enter"]),
    onKeydown: _withKeys($setup.onEsc, ["esc"])
  }, null, 32 /* NEED_HYDRATION */))
}
_sfc_main.render = render
export default _sfc_main
