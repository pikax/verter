import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createAssetComponent as _createAssetComponent, template as _template } from 'vue';
const t0 = _template("<p> ")
const t1 = _template("closed", 2)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  let s, s1
  const n4 = _createAssetComponent("Panel", null, {
    $: [
      () => (_ctx.open
        ? {
          name: "head",
          fn: s || (s = () => {
            const n0 = t0()
            const x0 = _txt(n0)
            _renderEffect(() => _setText(x0, _toDisplayString(_ctx.title)))
            return n0
          })
        }
        : {
          name: "foot",
          fn: s1 || (s1 = () => {
            const n2 = t1()
            return n2
          })
        })
    ]
  }, true)
  return n4
}
