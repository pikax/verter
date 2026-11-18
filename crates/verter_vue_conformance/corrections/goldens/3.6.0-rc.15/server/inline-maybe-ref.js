import { ssrRenderAttr as _ssrRenderAttr, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<!--[--><input${
    _ssrRenderAttr("value", $setup.value)
  }>${
    _ssrInterpolate($setup.value)
  }<!--]-->`)
}
