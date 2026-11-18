import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate, ssrRenderList as _ssrRenderList } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<pre${_ssrRenderAttrs(_attrs)}><!--[-->`)
  _ssrRenderList($setup.lines, (line) => {
    _push(`<span>${_ssrInterpolate(line)}
</span>`)
  })
  _push(`<!--]--></pre>`)
}
