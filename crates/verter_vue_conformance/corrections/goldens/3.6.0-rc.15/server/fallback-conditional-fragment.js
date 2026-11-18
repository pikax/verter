import { resolveComponent as _resolveComponent, withCtx as _withCtx, createVNode as _createVNode, Fragment as _Fragment, openBlock as _openBlock, createBlock as _createBlock, createCommentVNode as _createCommentVNode } from "vue"
import { ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        if ($setup.visible) {
          _push(`<!--[--><b${
            _scopeId
          }></b><i${
            _scopeId
          }></i><!--]-->`)
        } else {
          _push(`<!---->`)
        }
      } else {
        return [
          ($setup.visible)
            ? (_openBlock(), _createBlock(_Fragment, { key: 0 }, [
                _createVNode("b"),
                _createVNode("i")
              ], 64 /* STABLE_FRAGMENT */))
            : _createCommentVNode("v-if", true)
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
