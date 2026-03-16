import {openBlock as _openBlock,createElementVNode as _createElementVNode,createVNode as _createVNode,createCommentVNode as _createCommentVNode,createTextVNode as _createTextVNode,withCtx as _withCtx,mergeProps as _mergeProps,withDirectives as _withDirectives,resolveComponent as _resolveComponent,withKeys as _withKeys,createBlock as _createBlock,resolveDirective as _resolveDirective,normalizeClass as _normalizeClass,normalizeStyle as _normalizeStyle,vShow as _vShow} from 'vue';
import { ClickOutside as vClickOutside } from '@element-plus/directives'
const __sfc__ = /*@__PURE__*/{
__name: 'test',setup(__props,{expose:__expose}){__expose();

const triggerRef = ref()
const showPicker = ref(false)
const panelProps = computed(() => ({}))
const handleClickOutside = () => {}
const handleEsc = (e) => {}
const clearable = ref(true)
const btnKls = computed(() => [])
const buttonId = ref('')
const modelValue = ref('')
const showPanelColor = ref(false)
const showAlpha = ref(false)
const displayedColor = computed(() => '')
const ns = { be: (a, b) => '', is: (a, b) => '' }

const __returned__={vClickOutside, triggerRef, showPicker, panelProps, handleClickOutside, handleEsc, clearable, btnKls, buttonId, modelValue, showPanelColor, showAlpha, displayedColor, ns}
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}};function render(_ctx, _cache, $props, $setup, $data, $options) {const _component_el_tooltip = _resolveComponent("el-tooltip");
const _component_el_color_picker_panel = _resolveComponent("el-color-picker-panel");
const _component_el_button = _resolveComponent("el-button");
const _component_el_icon = _resolveComponent("el-icon");
const _component_arrow_down = _resolveComponent("arrow-down");
const _component_close = _resolveComponent("close");
const _directive_click_outside = _resolveDirective("click-outside");
return (_openBlock(), _createBlock(_component_el_tooltip, {ref: "popper", visible: $setup.showPicker, "show-arrow": false, trigger: "click"}, {content: _withCtx(() => [_withDirectives(_createVNode(_component_el_color_picker_panel, _mergeProps({ref: "pickerPanelRef"}, $setup.panelProps, {border: false, onKeydown: _withKeys($setup.handleEsc, ["esc"])}), {footer: _withCtx(() => [_createElementVNode("div", null, [($setup.clearable) ? (_openBlock(), _createBlock(_component_el_button, {key: 0, class: _normalizeClass($setup.ns.be('footer', 'link-btn')), text: "", size: "small", onClick: _ctx.clear}, {default: _withCtx(() => [_createTextVNode(" Clear ")]), _: 1 /* STABLE */}, 10 /* CLASS, PROPS */, ["onClick"])) : _createCommentVNode("v-if", true), _createVNode(_component_el_button, {plain: "", size: "small", class: _normalizeClass($setup.ns.be('footer', 'btn')), onClick: _ctx.confirmValue}, {default: _withCtx(() => [_createTextVNode(" Confirm ")]), _: 1 /* STABLE */}, 10 /* CLASS, PROPS */, ["onClick"])])]), _: 1 /* STABLE */}, 536 /* PROPS, FULL_PROPS, NEED_PATCH */, ["border", "onKeydown"]), [[_directive_click_outside, $setup.handleClickOutside, [$setup.triggerRef]]])]), default: _withCtx(() => [_createElementVNode("div", _mergeProps({id: $setup.buttonId, ref: "triggerRef"}, _ctx.$attrs, {class: _normalizeClass($setup.btnKls), role: "button"}), [_createElementVNode("div", {class: _normalizeClass($setup.ns.be('picker', 'trigger'))}, [_createElementVNode("span", {class: _normalizeClass([$setup.ns.be('picker', 'color'), $setup.ns.is('alpha', $setup.showAlpha)])}, [_createElementVNode("span", {class: _normalizeClass($setup.ns.be('picker', 'color-inner')), style: _normalizeStyle({ backgroundColor: $setup.displayedColor })}, [_withDirectives(_createVNode(_component_el_icon, {class: _normalizeClass([$setup.ns.be('picker', 'icon'), $setup.ns.is('icon-arrow-down')])}, {default: _withCtx(() => [_createVNode(_component_arrow_down, null)]), _: 1 /* STABLE */}, 514 /* CLASS, NEED_PATCH */), [[_vShow, $setup.modelValue || $setup.showPanelColor]]), _withDirectives(_createVNode(_component_el_icon, {class: _normalizeClass([$setup.ns.be('picker', 'empty'), $setup.ns.is('icon-close')])}, {default: _withCtx(() => [_createVNode(_component_close, null)]), _: 1 /* STABLE */}, 514 /* CLASS, NEED_PATCH */), [[_vShow, !$setup.modelValue && !$setup.showPanelColor]])], 6 /* CLASS, STYLE */)], 2 /* CLASS */)], 2 /* CLASS */)], 538 /* CLASS, PROPS, FULL_PROPS, NEED_PATCH */, ["id"])]), _: 1 /* STABLE */}, 520 /* PROPS, NEED_PATCH */, ["visible", "show-arrow"]))}
export default __sfc__;
