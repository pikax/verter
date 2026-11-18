import { renderSlot as _renderSlot, toDisplayString as _toDisplayString, createTextVNode as _createTextVNode, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"


const _sfc_main = {
  __name: 'child-comp',
  props: {
  label: { type: String, required: true },
  count: { type: Number, default: 0 },
},
  emits: ["select"],
  setup(__props, { emit: __emit }) {


const emit = __emit

function onClick() {
  emit("select")
}

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("button", {
    class: "child-comp",
    onClick: onClick
  }, [
    _renderSlot(_ctx.$slots, "header"),
    _createTextVNode(" " + _toDisplayString(__props.label) + " (" + _toDisplayString(__props.count) + ") ", 1 /* TEXT */),
    _renderSlot(_ctx.$slots, "default"),
    _renderSlot(_ctx.$slots, "footer", { total: __props.count })
  ]))
}
}

}
export default _sfc_main
