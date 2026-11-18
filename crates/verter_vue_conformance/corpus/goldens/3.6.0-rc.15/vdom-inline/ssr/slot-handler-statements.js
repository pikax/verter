import { defineComponent as _defineComponent } from "vue";
import { createElementVNode as _createElementVNode, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue";
import { ref } from "vue";
import ChildComp from "../components/child-comp.vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "slot-handler-statements",
  setup(__props) {
    const a = ref("");
    const b = ref("");
    return (_ctx, _cache) => {
      return _openBlock(), _createBlock(ChildComp, null, {
        default: _withCtx(() => [
          _createElementVNode("button", {
            onClick: _cache[0] || (_cache[0] = ($event) => {
              a.value = "";
              b.value = "";
            })
          }, "reset")
        ]),
        _: 1
        /* STABLE */
      });
    };
  }
});

export default _sfc_main
