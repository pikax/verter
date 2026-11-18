import { useCssVars as _useCssVars, defineComponent as _defineComponent } from "vue";
import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue";
const _hoisted_1 = { class: "a" };
import { ref } from "vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "css-vars-root",
  setup(__props) {
    _useCssVars((_ctx) => ({
      "89d5a0b5-color": color.value
    }));
    const color = ref("red");
    return (_ctx, _cache) => {
      return _openBlock(), _createElementBlock("p", _hoisted_1, "text");
    };
  }
});

export default _sfc_main
