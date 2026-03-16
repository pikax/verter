const body = `_push(_ssrRenderComponent(_component_a_form_item, _mergeProps({key: domain.key}, {ref_for: true}, index === 0 ? _ctx.formItemLayout : {}, {label: index === 0 ? 'Domains' : '', name: ['domains', index, 'value'], rules: {required: true, message: 'domain can not be null', trigger: 'change'}}), {default: _withCtx((_, _push, _parent) => {if (_push) {_push('hello')}}), _: 1}, _parent))`;

let s = body.replace(/\s+/g, " ");

// Key strip
s = s.replace(/\{\s*key:\s*\d+\s*\}/g, '{}');
s = s.replace(/\{\s*key:\s*\d+,\s*/g, '{ ');
s = s.replace(/,\s*key:\s*\d+/g, '');
s = s.replace(/,\s*\{\s*key:\s*[^}]+\}/g, '');
s = s.replace(/\{\s*key:\s*[\w$.[\]]+,\s*/g, '{ ');
s = s.replace(/,\s*key:\s*[\w$.[\]]+/g, '');
console.log('After key strip OK:', s.includes('label'));

// id strip
s = s.replace(/,\s*\{\s*id:\s*_ctx\.\w+\s*\}/g, "");
s = s.replace(/,\s*id:\s*_ctx\.\w+/g, "");
console.log('After id strip:', s.includes('label') ? 'OK' : 'LOST');

// tabindex strip
s = s.replace(/,\s*tabindex:\s*[^,}]+/g, "");
console.log('After tabindex:', s.includes('label') ? 'OK' : 'LOST');

// mergeProps unwrap
s = s.replace(/_mergeProps\(_ctx\.\w+,\s*(\{[^}]+\})\)/g, "$1");
console.log('After mergeProps unwrap:', s.includes('label') ? 'OK' : 'LOST');

// stripSingleArgMergeProps
function stripSingleArgMergeProps(s) {
  const needle = '_mergeProps(';
  let result = '';
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) { result += s.slice(i); break; }
    result += s.slice(i, idx);
    let j = idx + needle.length;
    let depth = 1, commaCount = 0;
    for (let k = j; k < s.length && depth > 0; k++) {
      const ch = s[k];
      if (ch === '(' || ch === '[' || ch === '{') depth++;
      else if (ch === ')' || ch === ']' || ch === '}') { depth--; if (depth === 0) { j = k + 1; break; } }
      else if (ch === ',' && depth === 1) commaCount++;
    }
    const inner = s.slice(idx + needle.length, j - 1).trim();
    if (commaCount === 0) { result += inner; } else { result += needle + inner + ')'; }
    i = j;
  }
  return result;
}
s = stripSingleArgMergeProps(s);
console.log('After stripSingle:', s.includes('label') ? 'OK' : 'LOST');

// Ref_for strip (separate objects)  
s = s.replace(/,\s*\{\s*ref_for:\s*true\s*\}/g, "");
console.log('After ref_for:', s.includes('label') ? 'OK' : 'LOST');

// Empty obj
s = s.replace(/,\s*\{\s*\}/g, "");
console.log('After empty obj:', s.includes('label') ? 'OK' : 'LOST');

// _ssrRenderAttrs  
s = s.replace(/_ssrRenderAttrs\(([^)]+),\s*"[^"]+"\)/g, "_ssrRenderAttrs($1)");
console.log('After ssrRenderAttrs:', s.includes('label') ? 'OK' : 'LOST');

// Sort mergeProps
// Import from normalize
import { normalizeForComparison } from './normalize.mjs';
// Can't easily extract sortMergePropsArgs, let me check post-sort by running full normalize

// Let me check what sortMergePropsArgs does
// Actually let me apply the remaining steps
// whitespace cleanup
s = s.replace(/\s+/g, " ");
s = s.replace(/\) }/g, ")}");
s = s.replace(/\{ /g, "{");
s = s.replace(/\(\s+/g, "(");
s = s.replace(/\s+\)/g, ")");
s = s.replace(/\s+\}/g, "}");
s = s.replace(/\s+\]/g, "]");
console.log('After whitespace cleanup:', s.includes('label') ? 'OK' : 'LOST');

// Fix leading/trailing commas
s = s.replace(/\{,\s*/g, "{");
s = s.replace(/,\s*\}/g, "}");
s = s.replace(/,\s*,/g, ",");
s = s.replace(/,\s*\{\s*\}/g, "");
console.log('After comma cleanup:', s.includes('label') ? 'OK' : 'LOST');

// stripSingleArgMergeProps again
s = stripSingleArgMergeProps(s);
console.log('After stripSingle2:', s.includes('label') ? 'OK' : 'LOST');

// NEW: strip trailing ]
s = s.replace(/\)(\])(,|\})/g, ")$2");
console.log('After trailing ]:', s.includes('label') ? 'OK' : 'LOST');

// style obj->str
s = s.replace(/\{style:\s*\{([^}]+)\}\}/g, (match, inner) => {
  const css = inner.replace(/(\w[\w-]*):\s*"([^"]*)"/g, '$1: $2').replace(/,\s*/g, '; ');
  return '{style: "' + css + '"}';
});
console.log('After style:', s.includes('label') ? 'OK' : 'LOST');

console.log('\n=== Current state ===');
console.log(s);
