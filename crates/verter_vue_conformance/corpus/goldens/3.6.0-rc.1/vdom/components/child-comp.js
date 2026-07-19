import { renderSlot as _renderSlot, toDisplayString as _toDisplayString, createTextVNode as _createTextVNode, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _sfc_main = {
  __name: 'child-comp',
  props: {
  label: { type: String, required: true },
  count: { type: Number, default: 0 },
},
  emits: ["select"],
  setup(__props, { expose: __expose, emit: __emit }) {
  __expose();


const emit = __emit

function onClick() {
  emit("select")
}

const __returned__ = { emit, onClick }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("button", {
    class: "child-comp",
    onClick: $setup.onClick
  }, [
    _renderSlot(_ctx.$slots, "header"),
    _createTextVNode(" " + _toDisplayString($props.label) + " (" + _toDisplayString($props.count) + ") ", 1 /* TEXT */),
    _renderSlot(_ctx.$slots, "default"),
    _renderSlot(_ctx.$slots, "footer", { total: $props.count })
  ]))
}
_sfc_main.render = render
export default _sfc_main
