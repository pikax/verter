import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createForSlots as _createForSlots, createAssetComponent as _createAssetComponent, template as _template } from 'vue';
const t0 = _template("<b> </b>")
const t1 = _template(" ")
const t2 = _template("<i> ")

export function render(_ctx, $props, $emit, $attrs, $slots) {
  let s
  const n6 = _createAssetComponent("Table", { rows: () => (_ctx.rows) }, {
    "head": (_slotProps0) => {
      const n0 = t0()
      const x0 = _txt(n0)
      _renderEffect(() => _setText(x0, _toDisplayString(_slotProps0.row.name)))
      return n0
    },
    $: [
      () => ({
        name: _ctx.name,
        fn: s || (s = (_slotProps0) => {
          const n4 = t2()
          const x4 = _txt(n4)
          _renderEffect(() => _setText(x4, _toDisplayString(_slotProps0.row.id)))
          return n4
        })
      }),
      _createForSlots(() => (_ctx.families), (_for_item0) => (_slotProps1) => {
        const n2 = t1()
        _renderEffect(() => {
          const _row = _slotProps1.row
          _setText(n2, _toDisplayString(_row.versions[_for_item0.value]))
        })
        return n2
      }, (family) => (`cell-${family}`))
    ]
  }, true)
  return n6
}
