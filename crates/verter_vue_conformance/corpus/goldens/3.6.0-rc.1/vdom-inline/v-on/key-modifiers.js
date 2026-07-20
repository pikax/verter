import { withKeys as _withKeys, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"


const _sfc_main = {
  __name: 'key-modifiers',
  setup(__props) {

function onEnter() {}
function onEsc() {}

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("input", {
    onKeyup: _withKeys(onEnter, ["enter"]),
    onKeydown: _withKeys(onEsc, ["esc"])
  }, null, 32 /* NEED_HYDRATION */))
}
}

}
export default _sfc_main
