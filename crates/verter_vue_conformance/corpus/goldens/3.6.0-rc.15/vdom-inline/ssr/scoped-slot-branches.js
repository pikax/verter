import { defineComponent as _defineComponent } from "vue";
import { openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString, createTextVNode as _createTextVNode, Fragment as _Fragment, withCtx as _withCtx, createBlock as _createBlock } from "vue";
const _hoisted_1 = { key: 0 };
import ChildComp from "../components/child-comp.vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "scoped-slot-branches",
  setup(__props) {
    return (_ctx, _cache) => {
      return _openBlock(), _createBlock(ChildComp, null, {
        cell: _withCtx(({ row }) => [
          row.a ? (_openBlock(), _createElementBlock("span", _hoisted_1, "a")) : (_openBlock(), _createElementBlock(
            _Fragment,
            { key: 1 },
            [
              _createTextVNode(
                _toDisplayString(row.b),
                1
                /* TEXT */
              )
            ],
            64
            /* STABLE_FRAGMENT */
          ))
        ]),
        _: 1
        /* STABLE */
      });
    };
  }
});

export default _sfc_main
