import { setInsertionState as _setInsertionState, createSlot as _createSlot, child as _child, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createInvoker as _createInvoker, delegateEvents as _delegateEvents, template as _template } from 'vue';

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
const t0 = _template("<button class=child-comp> ", 1)
_delegateEvents("click")

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n4 = t0()
  const n1 = _child(n4, 1)
  _setInsertionState(n4, 0)
  const n0 = _createSlot("header")
  _renderEffect(() => _setText(n1, " " + _toDisplayString($props.label) + " (" + _toDisplayString($props.count) + ") "))
  _setInsertionState(n4, null, 2)
  const n2 = _createSlot()
  _setInsertionState(n4, null, 3)
  const n3 = _createSlot("footer", { total: () => ($props.count) })
  n4.$evtclick = _createInvoker(_ctx.onClick)
  return n4
}
_sfc_main.render = render
export default _sfc_main
