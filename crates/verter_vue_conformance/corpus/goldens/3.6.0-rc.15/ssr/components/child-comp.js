
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
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderSlot as _ssrRenderSlot, ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<button${_ssrRenderAttrs(_mergeProps({ class: "child-comp" }, _attrs))}>`)
  _ssrRenderSlot(_ctx.$slots, "header", {}, null, _push, _parent)
  _push(` ${
    _ssrInterpolate($props.label)
  } (${
    _ssrInterpolate($props.count)
  }) `)
  _ssrRenderSlot(_ctx.$slots, "default", {}, null, _push, _parent)
  _ssrRenderSlot(_ctx.$slots, "footer", { total: $props.count }, null, _push, _parent)
  _push(`</button>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
