import { defineComponent as _defineComponent } from "vue";
import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue";
const _hoisted_1 = { class: "a" };
import { ref } from "vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "scoped-style-root",
  setup(__props) {
    const n = ref(1);
    return (_ctx, _cache) => {
      return _openBlock(), _createElementBlock(
        "p",
        _hoisted_1,
        _toDisplayString(n.value),
        1
        /* TEXT */
      );
    };
  }
});

export default _sfc_main
