import fs from 'fs';

const d = JSON.parse(fs.readFileSync('C:/temp/ssr-full-vmodel-merge.json', 'utf8'));
const mm = d.mismatches;

// For the Z-other category, try to find sub-patterns
const subcats = {};
const examples = {};

for (const m of mm) {
  const vueLines = (m.vue || '').split('\n');
  const verterLines = (m.verter || '').split('\n');

  // Quick filter: skip known categories
  const allText = (m.vue || '') + ' ||| ' + (m.verter || '');
  if (!m.verter || !m.verter.includes('ssrRender')) continue;
  if (allText.includes('_imports_') || allText.includes('_plugin_vue_export_helper')) continue;
  if (allText.includes('_ssrGetDirectiveProps')) continue;
  if (allText.includes('_ssrRenderSlotInner')) continue;
  if (allText.includes('Teleport') || allText.includes('_ssrRenderTeleport')) continue;
  if (allText.includes('_ssrRenderVNode')) continue;

  // Find first diff
  const diffs = [];
  const maxLen = Math.max(vueLines.length, verterLines.length);
  for (let i = 0; i < maxLen; i++) {
    const vl = (vueLines[i] || '').trim();
    const vrl = (verterLines[i] || '').trim();
    if (vl !== vrl) {
      diffs.push({ vue: vl, verter: vrl, lineIdx: i });
    }
  }
  if (diffs.length === 0) continue;

  // Already categorized ones
  const allDiffText = diffs.map(d => d.vue + ' ||| ' + d.verter).join('\n');
  if (allDiffText.includes('_mergeProps') || allDiffText.includes('mergeProps')) continue;
  if (allDiffText.includes('onUpdate:') || allDiffText.includes('v-model')) continue;
  if (allDiffText.includes('_ssrRenderAttr') && !allDiffText.includes('_ssrRenderAttrs')) continue;
  if (allDiffText.includes('_ssrRenderClass') || allDiffText.includes('_ssrRenderStyle')) continue;
  if (allDiffText.includes('_ssrRenderList') && !allDiffText.includes('_ssrRenderAttrs')) continue;
  if (allDiffText.includes('$slots')) continue;

  // Now try to identify sub-patterns for the remaining "other"
  const diff = diffs[0];
  const v = diff.vue, vr = diff.verter;
  let diffPos = 0;
  const minLen = Math.min(v.length, vr.length);
  while (diffPos < minLen && v[diffPos] === vr[diffPos]) diffPos++;
  const vueCtx = v.slice(Math.max(0, diffPos - 40), Math.min(v.length, diffPos + 60));
  const verterCtx = vr.slice(Math.max(0, diffPos - 40), Math.min(vr.length, diffPos + 60));

  let subcat = 'unknown';

  // Component casing
  if (/[a-z][A-Z]/.test(vueCtx.slice(vueCtx.length/2-10, vueCtx.length/2+10)) &&
      /[A-Z][a-z]/.test(verterCtx.slice(verterCtx.length/2-10, verterCtx.length/2+10))) {
    // Check if it's really just a casing difference at the diff point
    const vChar = v[diffPos] || '';
    const vrChar = vr[diffPos] || '';
    if (vChar.toLowerCase() === vrChar.toLowerCase()) {
      subcat = 'component-casing';
    }
  }

  if (subcat === 'unknown') {
    // Check slot ordering
    if (vueCtx.includes('_withCtx') || verterCtx.includes('_withCtx')) {
      // Check if it's slot name ordering
      const vueSlotMatch = vueCtx.match(/(\w+):\s*_withCtx/);
      const verterSlotMatch = verterCtx.match(/(\w+):\s*_withCtx/);
      if (vueSlotMatch && verterSlotMatch && vueSlotMatch[1] !== verterSlotMatch[1]) {
        subcat = 'slot-ordering';
      } else {
        subcat = 'slot-structure';
      }
    }
    // Conditional slot rendering
    else if (vueCtx.includes('? {') || verterCtx.includes('? {') ||
             vueCtx.includes(': {name:') || verterCtx.includes(': {name:')) {
      subcat = 'conditional-slot';
    }
    // v-for fragment markers
    else if (vueCtx.includes('<span>') || verterCtx.includes('<span>') ||
             vueCtx.includes('<!--[-->') || verterCtx.includes('<!--[-->')) {
      subcat = 'fragment-markers';
    }
    // Push content differences
    else if (vueCtx.includes('_push(') || verterCtx.includes('_push(')) {
      subcat = 'push-content';
    }
    // Style differences
    else if (vueCtx.includes('style') || verterCtx.includes('style')) {
      subcat = 'style-diff';
    }
    // Binding prefix
    else if (vueCtx.includes('$data.') || verterCtx.includes('$data.') ||
             vueCtx.includes('$props.') || verterCtx.includes('$props.') ||
             vueCtx.includes('$setup.') || verterCtx.includes('$setup.')) {
      subcat = 'binding-prefix';
    }
  }

  subcats[subcat] = (subcats[subcat] || 0) + 1;
  if (!examples[subcat]) examples[subcat] = [];
  if (examples[subcat].length < 3) {
    examples[subcat].push({
      file: m.file,
      vueCtx: vueCtx.slice(0, 100),
      verterCtx: verterCtx.slice(0, 100),
    });
  }
}

const sorted = Object.entries(subcats).sort((a,b) => b[1] - a[1]);
console.log('\n=== Z-other Sub-categories ===');
let total = 0;
for (const [cat, count] of sorted) {
  console.log(`  ${cat}: ${count}`);
  total += count;
}
console.log(`  TOTAL: ${total}`);

console.log('\n=== Examples ===');
for (const [cat, count] of sorted) {
  console.log(`\n--- ${cat} (${count}) ---`);
  for (const ex of examples[cat]) {
    console.log(`  File: ${ex.file}`);
    console.log(`    Vue:    ...${ex.vueCtx}...`);
    console.log(`    Verter: ...${ex.verterCtx}...`);
  }
}
