
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
import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<button${
    _ssrRenderAttrs(_attrs)
  }>${
    _ssrInterpolate($props.msg)
  }:${
    _ssrInterpolate($props.level)
  }</button>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
