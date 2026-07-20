import { txt as _txt, createInvoker as _createInvoker, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, delegateEvents as _delegateEvents, template as _template } from 'vue';
import { defineVaporComponent as _defineVaporComponent } from "vue";
const _sfc_main = /* @__PURE__ */ _defineVaporComponent({
  __name: "props-type-withdefaults",
  props: {
    item: { type: Object, required: true },
    size: { type: String, required: false, default: "sm" },
    tags: { type: Array, required: false, default: () => [] }
  },
  emits: ["pick", "close"],
  setup(__props, { expose: __expose, emit: __emit }) {
    __expose();
    const props = __props;
    const emit = __emit;
    const __returned__ = { props, emit };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

const t0 = _template("<div> ", 1)
_delegateEvents("click")

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  const x0 = _txt(n0)
  n0.$evtclick = _createInvoker(() => (_ctx.emit('pick', _ctx.props.item)))
  _renderEffect(() => _setText(x0, _toDisplayString($props.item.name) + " (" + _toDisplayString($props.size) + ") " + _toDisplayString($props.tags.length)))
  return n0
}
_sfc_main.render = render
export default _sfc_main
