import { resolveComponent as _resolveComponent, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createComponentWithFallback as _createComponentWithFallback, setProp as _setProp, createIf as _createIf, extend as _extend, createSlot as _createSlot, txt as _txt, setStaticTemplateRef as _setStaticTemplateRef, createForSlots as _createForSlots, template as _template } from 'vue';
const t0 = _template("H", 2)
const t1 = _template(" ")
const t2 = _template("<p>t")
const t3 = _template("<span> ")
const t4 = _template("A", 2)
const t5 = _template("B", 2)
const t6 = _template("C", 2)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const _component_Card = _resolveComponent("Card")
  const _component_Foo = _resolveComponent("Foo")
  const _component_Child = _resolveComponent("Child")
  const _component_Comp = _resolveComponent("Comp")
  const n3 = _createComponentWithFallback(_component_Card, null, {
    "header": () => {
      const n0 = t0()
      return n0
    },
    "default": () => {
      const n2 = t1()
      _renderEffect(() => _setText(n2, _toDisplayString(_ctx.body)))
      return n2
    }
  })
  const n7 = _createComponentWithFallback(_component_Card, null, {
    "header": () => {
      const n4 = t0()
      return n4
    },
    "default": () => {
      const n6 = t2()
      _renderEffect(() => _setProp(n6, "id", _ctx.id))
      return n6
    }
  })
  const n11 = _createComponentWithFallback(_component_Child, null, _extend(() => {
    const n8 = _createIf(() => (_ctx.ok), () => {
      const n10 = _createComponentWithFallback(_component_Foo, null, null, 2 /* SLOT */)
      return n10
    }, null, 129 /* TRUE_SINGLE_ROOT, SLOT_ROOT */)
    return n8
  }, { _: 1 /* NON_STABLE */ }))
  const n18 = _createComponentWithFallback(_component_Child, null, {
    "a": _extend(() => {
      const n12 = _createIf(() => (_ctx.ok), () => {
        const n14 = _createComponentWithFallback(_component_Foo, null, null, 2 /* SLOT */)
        return n14
      }, null, 129 /* TRUE_SINGLE_ROOT, SLOT_ROOT */)
      return n12
    }, { _: 1 /* NON_STABLE */ }),
    "b": _extend(() => {
      const n16 = _createSlot("default", null, null, 4 /* FORWARDED */)
      return n16
    }, { _: 1 /* NON_STABLE */ })
  })
  const n21 = _createComponentWithFallback(_component_Comp, null, {
    $: [
      _createForSlots(() => (_ctx.items), (_for_item0) => () => {
        const n19 = t3()
        const x19 = _txt(n19)
        _setStaticTemplateRef(n19, "row")
        _renderEffect(() => _setText(x19, _toDisplayString(_for_item0.value)))
        return n19
      }, (i) => (i))
    ]
  })
  let s, s1
  const n28 = _createComponentWithFallback(_component_Comp, null, {
    $: [
      () => ({
        name: _ctx.name,
        fn: s || (s = () => {
          const n24 = t5()
          return n24
        })
      }),
      {
        "c": () => {
          const n26 = t6()
          return n26
        }
      },
      () => (_ctx.ok
        ? {
          name: "a",
          fn: s1 || (s1 = () => {
            const n22 = t4()
            return n22
          })
        }
        : void 0)
    ]
  })
  return [n3, n7, n11, n18, n21, n28]
}
