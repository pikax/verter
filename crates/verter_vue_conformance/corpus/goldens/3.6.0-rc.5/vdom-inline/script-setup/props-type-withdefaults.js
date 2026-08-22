import { defineComponent as _defineComponent } from "vue";
import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "props-type-withdefaults",
  props: {
    item: { type: Object, required: true },
    size: { type: String, required: false, default: "sm" },
    tags: { type: Array, required: false, default: () => [] }
  },
  emits: ["pick", "close"],
  setup(__props, { emit: __emit }) {
    const props = __props;
    const emit = __emit;
    return (_ctx, _cache) => {
      return _openBlock(), _createElementBlock(
        "div",
        {
          onClick: _cache[0] || (_cache[0] = ($event) => emit("pick", props.item))
        },
        _toDisplayString(__props.item.name) + " (" + _toDisplayString(__props.size) + ") " + _toDisplayString(__props.tags.length),
        1
        /* TEXT */
      );
    };
  }
});

export default _sfc_main
