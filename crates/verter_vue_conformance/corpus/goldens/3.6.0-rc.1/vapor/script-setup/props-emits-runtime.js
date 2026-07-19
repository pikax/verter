import { txt as _txt, createInvoker as _createInvoker, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, delegateEvents as _delegateEvents, template as _template } from 'vue';

const _sfc_main = {
  __name: 'props-emits-runtime',
  props: {
  msg: { type: String, default: "hi" },
  level: { type: Number, required: true },
},
  emits: ["save", "cancel"],
  __vapor: true,
  setup(__props, { expose: __expose, emit: __emit }) {
  __expose();

const props = __props
const emit = __emit

function onSave() {
  emit("save", props.msg)
}

const __returned__ = { props, emit, onSave }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<button> ", 1)
_delegateEvents("click")

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  const x0 = _txt(n0)
  n0.$evtclick = _createInvoker(_ctx.onSave)
  _renderEffect(() => _setText(x0, _toDisplayString($props.msg) + ":" + _toDisplayString($props.level)))
  return n0
}
_sfc_main.render = render
export default _sfc_main
