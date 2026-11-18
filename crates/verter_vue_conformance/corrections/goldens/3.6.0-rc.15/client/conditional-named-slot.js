import { createElementVNode as _createElementVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, createSlots as _createSlots, openBlock as _openBlock, createBlock as _createBlock } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, _createSlots({ _: 2 /* DYNAMIC */ }, [
    ($setup.visible)
      ? {
          name: "extra",
          fn: _withCtx(() => [
            _cache[0] || (_cache[0] = _createElementVNode("b", null, null, -1 /* CACHED */))
          ]),
          key: "0"
        }
      : undefined
  ]), 1024 /* DYNAMIC_SLOTS */))
}
