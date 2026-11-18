import { defineComponent as _defineComponent } from "vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "props-type-withdefaults",
  props: {
    item: { type: Object, required: true },
    size: { type: String, required: false, default: "sm" },
    tags: { type: Array, required: false, default: () => [] }
  },
  emits: ["pick", "close"],
  setup(__props, { expose: __expose, emit: __emit }) {
    __expose();
    const props = __props;
    const emit = __emit;
    const __returned__ = { props, emit };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${
    _ssrRenderAttrs(_attrs)
  }>${
    _ssrInterpolate($props.item.name)
  } (${
    _ssrInterpolate($props.size)
  }) ${
    _ssrInterpolate($props.tags.length)
  }</div>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
