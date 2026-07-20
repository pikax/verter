import { withModifiers as _withModifiers, createElementVNode as _createElementVNode, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"


const _sfc_main = {
  __name: 'modifiers',
  setup(__props) {

function save() {}
function del() {}
function open() {}

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", {
    onClick: _withModifiers(save, ["self"])
  }, [
    _createElementVNode("button", {
      onClick: _withModifiers(del, ["stop","prevent"])
    }, "Delete"),
    _createElementVNode("a", {
      href: "/x",
      onClickOnceCapture: open
    }, "Open", 32 /* NEED_HYDRATION */)
  ]))
}
}

}
export default _sfc_main
