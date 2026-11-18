import { defineComponent as _defineComponent } from "vue";
import { createTextVNode as _createTextVNode, withCtx as _withCtx, createSlots as _createSlots, openBlock as _openBlock, createBlock as _createBlock } from "vue";
import ChildComp from "../components/child-comp.vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "conditional-slot-static",
  setup(__props) {
    return (_ctx, _cache) => {
      return _openBlock(), _createBlock(
        ChildComp,
        null,
        _createSlots({
          _: 2
          /* DYNAMIC */
        }, [
          true ? {
            name: "actions",
            fn: _withCtx(() => [
              _cache[0] || (_cache[0] = _createTextVNode(
                "x",
                -1
                /* CACHED */
              ))
            ]),
            key: "0"
          } : void 0
        ]),
        1024
        /* DYNAMIC_SLOTS */
      );
    };
  }
});

export default _sfc_main
