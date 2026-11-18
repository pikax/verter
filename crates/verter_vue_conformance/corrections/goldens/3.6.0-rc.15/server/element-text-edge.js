import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${
    _ssrRenderAttrs(_attrs)
  }>${
    _ssrInterpolate($setup.label)
  }</div>`)
}
