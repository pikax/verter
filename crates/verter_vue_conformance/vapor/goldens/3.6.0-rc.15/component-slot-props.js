import { resolveComponent as _resolveComponent, createComponentWithFallback as _createComponentWithFallback, setProp as _setProp, renderEffect as _renderEffect, applyTextModel as _applyTextModel, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, createAssetComponent as _createAssetComponent, template as _template } from 'vue';
const t0 = _template("<input>")
const t1 = _template("<b> ")

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const _component_TextInput = _resolveComponent("TextInput")
  const _component_Field = _resolveComponent("Field")
  const n1 = _createComponentWithFallback(_component_Field, null, (field) => {
    const _on_update_modelValue = _value => (_ctx.name = _value)
    const n0 = _createComponentWithFallback(_component_TextInput, {
      id: () => (field.id),
      modelValue: () => (_ctx.name),
      "onUpdate:modelValue": () => _on_update_modelValue,
      describedby: () => (field.describedby)
    }, null, 2 /* SLOT */)
    return n0
  })
  const n3 = _createComponentWithFallback(_component_Field, null, (_slotProps0) => {
    const n2 = t0()
    _renderEffect(() => {
      _setProp(n2, "id", _slotProps0.id)
      _setProp(n2, "title", _slotProps0.hint)
    })
    _applyTextModel(n2, () => (_ctx.name), _value => (_ctx.name = _value))
    return n2
  })
  const n6 = _createAssetComponent("Table", { rows: () => (_ctx.rows) }, {
    "cell": (_slotProps0) => {
      const n4 = t1()
      const x4 = _txt(n4)
      _renderEffect(() => _setText(x4, _toDisplayString(_slotProps0.row.name)))
      return n4
    }
  })
  return [n1, n3, n6]
}
