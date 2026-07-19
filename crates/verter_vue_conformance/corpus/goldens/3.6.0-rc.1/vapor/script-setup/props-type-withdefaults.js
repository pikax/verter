import { defineVaporComponent as _defineVaporComponent } from "vue";
import { txt as _txt, createInvoker as _createInvoker, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, delegateEvents as _delegateEvents, template as _template } from "vue";
const t0 = _template("<div> ", 1);
_delegateEvents("click");
export default /* @__PURE__ */ _defineVaporComponent({
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
    const n0 = t0();
    const x0 = _txt(n0);
    n0.$evtclick = _createInvoker(() => emit("pick", props.item));
    _renderEffect(() => _setText(x0, _toDisplayString(__props.item.name) + " (" + _toDisplayString(__props.size) + ") " + _toDisplayString(__props.tags.length)));
    return n0;
  }
});
