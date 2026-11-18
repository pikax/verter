import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate, ssrRenderList as _ssrRenderList } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${_ssrRenderAttrs(_attrs)}><!--[-->`)
  _ssrRenderList($setup.rows, (row) => {
    _push(`<div><button>${_ssrInterpolate(row.label)}</button></div>`)
  })
  _push(`<!--]--></div>`)
}
