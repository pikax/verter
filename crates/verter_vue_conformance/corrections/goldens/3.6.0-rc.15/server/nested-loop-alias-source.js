import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate, ssrRenderList as _ssrRenderList } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${_ssrRenderAttrs(_attrs)}><!--[-->`)
  _ssrRenderList($setup.groups, (row, i) => {
    _push(`<div><!--[-->`)
    _ssrRenderList(row, (cell) => {
      _push(`<span>${_ssrInterpolate(cell)}</span>`)
    })
    _push(`<!--]--></div>`)
  })
  _push(`<!--]--></div>`)
}
