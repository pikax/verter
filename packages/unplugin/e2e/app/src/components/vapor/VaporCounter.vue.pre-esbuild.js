import { defineComponent as _defineComponent } from "vue";
import { ref } from "vue";
import {
  template as _template,
  setText as _setText,
  delegateEvents as _delegateEvents,
  renderEffect as _renderEffect,
  toDisplayString as _toDisplayString,
  txt as _txt,
  createInvoker as _createInvoker,
  child as _child,
  next as _next,
} from "vue";

const _sfc_main = /*@__PURE__*/ _defineComponent({
  __name: "VaporCounter",
  __vapor: true,
  setup(__props) {
    const count = ref(0);

    function increment() {
      count.value++;
    }

    return { count, increment };
  },
});

const t0 = _template(
  '<div data-testid="vapor-counter"><span data-testid="vapor-count"> </span><button data-testid="vapor-increment">+1',
);
_delegateEvents("click");

function render(_ctx) {
  const n0 = t0();
  const n1 = _child(n0);
  const n2 = _next(n1, 1);
  const x0 = _txt(n1);
  _renderEffect(() => _setText(x0, _toDisplayString(_ctx.count)));
  n2.$evtclick = _createInvoker((e) => _ctx.increment(e));

  return n0;
}

_sfc_main.render = render;

export default _sfc_main;
