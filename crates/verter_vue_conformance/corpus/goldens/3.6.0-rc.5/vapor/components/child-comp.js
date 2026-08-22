
const _sfc_main = {
  __name: 'child-comp',
  props: {
  label: { type: String, required: true },
  count: { type: Number, default: 0 },
},
  emits: ["select"],
  __vapor: true,
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
import { child as _child, setInsertionState as _setInsertionState, createSlot as _createSlot, next as _next, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, on as _on, template as _template } from 'vue';
const t0 = _template("<button class=child-comp><!> ", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n5 = t0()
  const n4 = _child(n5)
  const n1 = _next(n4)
  _setInsertionState(n5, n4)
  const n0 = _createSlot("header")
  _renderEffect(() => _setText(n1, " " + _toDisplayString($props.label) + " (" + _toDisplayString($props.count) + ") "))
  _setInsertionState(n5, 2)
  const n2 = _createSlot()
  _setInsertionState(n5, 3)
  const n3 = _createSlot("footer", { total: () => ($props.count) })
  _on(n5, "click", _ctx.onClick)
  return n5
}
_sfc_main.render = render
export default _sfc_main
