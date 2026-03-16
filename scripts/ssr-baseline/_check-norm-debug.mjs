import { extractSsrRenderBody, normalizeForComparison, extractImports } from './normalize.mjs';

const verterRaw = `function ssrRender(_ctx, _push, _parent, _attrs) {
_push(\`<div\${_ssrRenderAttrs(_mergeProps({ class: "relative flex flex-row items-center justify-start" }, _attrs))}><input\${_ssrRenderAttrs(_mergeProps({ ..._ctx.$attrs, onChange: $setup.updateValue }, { id: $setup.uuid, checked: $props.modelValue, class: "h-5.5 w-5.5 peer mb-0 cursor-pointer appearance-none rounded-sm border border-menu-selection bg-layer-0 focus-brand", type: "checkbox" }))}><div class="pointer-events-none absolute left-[0.2rem] hidden h-4 w-4 rounded-sm bg-menu-selection peer-checked:block"></div>\`)
if ($props.label) {
_push(\`<label class="ml-2 cursor-pointer select-none text-lg"\${_ssrRenderAttr("for", $setup.uuid)}>\${_ssrInterpolate($props.label)}</label>\`)
} else {
_push(\`<!---->\`)
}
_push(\`</div>\`)
}`;

const body = extractSsrRenderBody(verterRaw);
console.log('--- Extracted body:');
console.log(body);

const imports = new Map();
const normalized = normalizeForComparison(body, imports);
console.log('\n--- Normalized:');
console.log(normalized);
console.log('\nHas "id:" in normalized:', normalized.includes('id:'));
console.log('Has "id" in normalized:', normalized.includes('id'));
