import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _sfc_main = {
  __name: 'props-emits-runtime',
  props: {
  msg: { type: String, default: "hi" },
  level: { type: Number, required: true },
},
  emits: ["save", "cancel"],
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
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("button", { onClick: $setup.onSave }, _toDisplayString($props.msg) + ":" + _toDisplayString($props.level), 1 /* TEXT */))
}
_sfc_main.render = render
export default _sfc_main
