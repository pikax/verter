import { ssrRenderAttr as _ssrRenderAttr, ssrRenderAttrs as _ssrRenderAttrs, ssrRenderList as _ssrRenderList } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${_ssrRenderAttrs(_attrs)}><!--[-->`)
  _ssrRenderList($setup.rows, (row) => {
    _push(`<div><button>x</button><button>y</button><input${_ssrRenderAttr("value", $setup.draft[row])}></div>`)
  })
  _push(`<!--]--></div>`)
}
