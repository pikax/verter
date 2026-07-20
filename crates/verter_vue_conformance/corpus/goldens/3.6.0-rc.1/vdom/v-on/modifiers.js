import { withModifiers as _withModifiers, createElementVNode as _createElementVNode, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _sfc_main = {
  __name: 'modifiers',
  setup(__props, { expose: __expose }) {
  __expose();

function save() {}
function del() {}
function open() {}

const __returned__ = { save, del, open }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", {
    onClick: _withModifiers($setup.save, ["self"])
  }, [
    _createElementVNode("button", {
      onClick: _withModifiers($setup.del, ["stop","prevent"])
    }, "Delete"),
    _createElementVNode("a", {
      href: "/x",
      onClickOnceCapture: $setup.open
    }, "Open", 32 /* NEED_HYDRATION */)
  ]))
}
_sfc_main.render = render
export default _sfc_main
