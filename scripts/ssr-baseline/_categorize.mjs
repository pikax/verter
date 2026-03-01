import fs from 'fs';

const d = JSON.parse(fs.readFileSync('C:/temp/ssr-full-vmodel-merge.json', 'utf8'));
const mm = d.mismatches;

// For each mismatch, look at the first differing line to categorize
const cats = {};
const examples = {};

for (const m of mm) {
  const vueLines = (m.vue || '').split('\n');
  const verterLines = (m.verter || '').split('\n');

  // Find first differing line
  let firstDiffVue = '';
  let firstDiffVerter = '';
  for (let i = 0; i < Math.max(vueLines.length, verterLines.length); i++) {
    const vl = (vueLines[i] || '').trim();
    const vrl = (verterLines[i] || '').trim();
    if (vl !== vrl) {
      firstDiffVue = vl;
      firstDiffVerter = vrl;
      break;
    }
  }

  const diff = firstDiffVue + ' ||| ' + firstDiffVerter;
  let cat = 'Z-other';

  if (!m.verter || m.verter.includes('function ssrRender(') === false) {
    if (!m.verter || m.verter.trim() === '') cat = 'A-no-ssrRender-empty';
    else if (m.verter.includes('ssrRender') === false) cat = 'A-no-ssrRender';
  }

  if (cat === 'Z-other') {
    if (firstDiffVue.includes('_imports_') || firstDiffVerter.includes('_imports_') ||
        (firstDiffVue.includes('import ') && firstDiffVue.includes('_plugin_vue_export_helper'))) {
      cat = 'B-asset-imports';
    } else if (firstDiffVue.includes('_mergeProps') || firstDiffVerter.includes('_mergeProps') ||
               firstDiffVue.includes('mergeProps') || firstDiffVerter.includes('mergeProps')) {
      cat = 'D-mergeProps';
    } else if (firstDiffVue.includes('ssrGetDirectiveProps') || firstDiffVerter.includes('Directive')) {
      cat = 'C-custom-directive';
    } else if (firstDiffVue.includes('_ssrRenderVNode') || firstDiffVerter.includes('_ssrRenderVNode')) {
      cat = 'F-ssrRenderVNode';
    } else if (firstDiffVue.includes('v-model') || firstDiffVerter.includes('v-model') ||
               firstDiffVue.includes('onUpdate:model') || firstDiffVerter.includes('onUpdate:model')) {
      cat = 'E-v-model';
    } else if (diff.includes('Teleport') || diff.includes('teleport')) {
      cat = 'G-teleport';
    } else if (firstDiffVue.includes('_ssrRenderSlotInner') || firstDiffVerter.includes('_ssrRenderSlotInner')) {
      cat = 'H-slotInner';
    } else if (/[a-z][A-Z]/.test(firstDiffVue) && /[A-Z][a-z]/.test(firstDiffVerter)) {
      // Casing differences
      cat = 'I-component-casing';
    }
  }

  cats[cat] = (cats[cat] || 0) + 1;
  if (!examples[cat]) examples[cat] = [];
  if (examples[cat].length < 3) {
    examples[cat].push({ file: m.file, diffVue: firstDiffVue.slice(0,120), diffVerter: firstDiffVerter.slice(0,120) });
  }
}

// Sort by count descending
const sorted = Object.entries(cats).sort((a,b) => b[1] - a[1]);
console.log('\n=== Mismatch Categories ===');
for (const [cat, count] of sorted) {
  console.log(`  ${cat}: ${count}`);
}

console.log('\n=== Examples ===');
for (const [cat, count] of sorted) {
  console.log(`\n--- ${cat} (${count}) ---`);
  for (const ex of examples[cat]) {
    console.log(`  File: ${ex.file}`);
    console.log(`    Vue:    ${ex.diffVue}`);
    console.log(`    Verter: ${ex.diffVerter}`);
  }
}
