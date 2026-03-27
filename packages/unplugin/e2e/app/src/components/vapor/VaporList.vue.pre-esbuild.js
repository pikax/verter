import { defineComponent as _defineComponent } from "vue";
import { ref } from "vue";
import {
  template as _template,
  setText as _setText,
  setProp as _setProp,
  delegateEvents as _delegateEvents,
  createFor as _createFor,
  renderEffect as _renderEffect,
  toDisplayString as _toDisplayString,
  txt as _txt,
  createInvoker as _createInvoker,
  child as _child,
  next as _next,
  setInsertionState as _setInsertionState,
} from "vue";

const _sfc_main = /*@__PURE__*/ _defineComponent({
  __name: "VaporList",
  __vapor: true,
  setup(__props) {
    const items = ref(["Alpha", "Beta", "Gamma"]);

    function addItem() {
      items.value.push(`Item ${items.value.length + 1}`);
    }

    return { items, addItem };
  },
});

const t0 = _template("<li> ");
const t1 = _template("");
_delegateEvents("click");

function render(_ctx) {
  const n0 = t1();
  _setInsertionState(p0, null, 0, true);
  const n3 = _createFor(
    () => _ctx.items,
    (_for_item0, _for_key0) => {
      const n4 = t0();
      const x0 = _txt(n4);
      const n1 = _child(n0);
      const p0 = _next(n1, 1);
      _renderEffect(() => {
        _setProp(n4, "key", _for_key0.value);
        _setText(x0, _toDisplayString(_for_item0.value));
      });
      n1.$evtclick = _createInvoker((e) => _ctx.addItem(e));
      return n4;
    },
    (item, index) => index,
  );

  return n0;
}

_sfc_main.render = render;

export default _sfc_main;
