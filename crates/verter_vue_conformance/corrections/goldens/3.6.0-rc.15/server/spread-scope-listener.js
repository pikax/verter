import { resolveComponent as _resolveComponent, mergeProps as _mergeProps, withCtx as _withCtx, createVNode as _createVNode } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs, ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    default: _withCtx(({ item }, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<button${
          _ssrRenderAttrs(_mergeProps($setup.attrs, { title: $setup.label }))
        }${
          _scopeId
        }>x</button><p${
          _ssrRenderAttrs(_mergeProps($setup.attrs, { ref: $setup.setBox }))
        }${
          _scopeId
        }>y</p>`)
      } else {
        return [
          _createVNode("button", _mergeProps($setup.attrs, {
            onClick: $event => ($setup.pick(item)),
            title: $setup.label
          }), "x", 16 /* FULL_PROPS */, ["onClick", "title"]),
          _createVNode("p", _mergeProps($setup.attrs, { ref: $setup.setBox }), "y", 16 /* FULL_PROPS */)
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
