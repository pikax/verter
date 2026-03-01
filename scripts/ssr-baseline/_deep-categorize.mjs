import fs from 'fs';

const d = JSON.parse(fs.readFileSync('C:/temp/ssr-full-vmodel-merge.json', 'utf8'));
const mm = d.mismatches;

// Better categorization: look at ALL differing lines, not just the first
const cats = {};
const examples = {};

for (const m of mm) {
  const vueLines = (m.vue || '').split('\n');
  const verterLines = (m.verter || '').split('\n');

  // Find ALL differing lines
  const diffs = [];
  const maxLen = Math.max(vueLines.length, verterLines.length);
  for (let i = 0; i < maxLen; i++) {
    const vl = (vueLines[i] || '').trim();
    const vrl = (verterLines[i] || '').trim();
    if (vl !== vrl) {
      diffs.push({ vue: vl, verter: vrl, lineIdx: i });
    }
  }

  if (diffs.length === 0) continue; // shouldn't happen

  let cat = 'Z-other';

  // Check if verter has no output
  if (!m.verter || m.verter.trim() === '' || !m.verter.includes('ssrRender')) {
    cat = 'A-no-ssrRender';
  } else {
    // Collect all diff text for pattern matching
    const allDiffText = diffs.map(d => d.vue + ' ||| ' + d.verter).join('\n');

    // Try to categorize based on diff patterns
    if (allDiffText.includes('_imports_') || allDiffText.includes('_plugin_vue_export_helper')) {
      cat = 'B-asset-imports';
    } else if (allDiffText.includes('_ssrGetDirectiveProps')) {
      cat = 'C-custom-directive';
    } else if (allDiffText.includes('_ssrRenderSlotInner')) {
      cat = 'H-slotInner';
    } else if (allDiffText.includes('Teleport') || allDiffText.includes('_ssrRenderTeleport')) {
      cat = 'G-teleport';
    } else if (allDiffText.includes('_ssrRenderVNode')) {
      cat = 'F-ssrRenderVNode';
    }

    if (cat === 'Z-other') {
      // Look at individual diff patterns more carefully
      for (const diff of diffs) {
        const v = diff.vue;
        const vr = diff.verter;

        // Find the first character position where they differ
        let diffPos = 0;
        const minLen = Math.min(v.length, vr.length);
        while (diffPos < minLen && v[diffPos] === vr[diffPos]) diffPos++;

        // Get context around the diff point
        const ctxStart = Math.max(0, diffPos - 30);
        const ctxEnd = Math.min(v.length, diffPos + 30);
        const vueCtx = v.slice(ctxStart, ctxEnd);
        const verterCtx = vr.slice(Math.max(0, diffPos - 30), Math.min(vr.length, diffPos + 30));

        if (vueCtx.includes('_mergeProps') || verterCtx.includes('_mergeProps') ||
            vueCtx.includes('mergeProps') || verterCtx.includes('mergeProps')) {
          cat = 'D-mergeProps';
          break;
        }
        if (vueCtx.includes('onUpdate:') || verterCtx.includes('onUpdate:') ||
            vueCtx.includes('v-model') || verterCtx.includes('v-model')) {
          cat = 'E-v-model';
          break;
        }
      }
    }

    // If still other, look at specific char-level diff patterns
    if (cat === 'Z-other') {
      for (const diff of diffs) {
        const v = diff.vue;
        const vr = diff.verter;
        let diffPos = 0;
        const minLen = Math.min(v.length, vr.length);
        while (diffPos < minLen && v[diffPos] === vr[diffPos]) diffPos++;

        const vueCtx = v.slice(Math.max(0, diffPos - 50), Math.min(v.length, diffPos + 50));
        const verterCtx = vr.slice(Math.max(0, diffPos - 50), Math.min(vr.length, diffPos + 50));

        // Check for _ssrRenderAttr vs inline attr
        if (vueCtx.includes('_ssrRenderAttr') || verterCtx.includes('_ssrRenderAttr')) {
          cat = 'J-ssrRenderAttr';
          break;
        }
        // Check for v-show
        if (vueCtx.includes('v-show') || verterCtx.includes('v-show') ||
            vueCtx.includes('display:') || verterCtx.includes('display:')) {
          cat = 'K-v-show';
          break;
        }
        // Check for slot-related diffs
        if (vueCtx.includes('_ssrRenderSlot') || verterCtx.includes('_ssrRenderSlot') ||
            vueCtx.includes('$slots') || verterCtx.includes('$slots')) {
          cat = 'L-slot-diff';
          break;
        }
        // Check for v-for
        if (vueCtx.includes('_ssrRenderList') || verterCtx.includes('_ssrRenderList')) {
          cat = 'M-v-for';
          break;
        }
        // Check for class/style
        if (vueCtx.includes('_ssrRenderClass') || verterCtx.includes('_ssrRenderClass') ||
            vueCtx.includes('_ssrRenderStyle') || verterCtx.includes('_ssrRenderStyle')) {
          cat = 'N-class-style';
          break;
        }
      }
    }
  }

  cats[cat] = (cats[cat] || 0) + 1;
  if (!examples[cat]) examples[cat] = [];
  if (examples[cat].length < 3) {
    const diff = diffs[0];
    // Show the char-level diff context
    let diffPos = 0;
    const v = diff.vue, vr = diff.verter;
    const minLen = Math.min(v.length, vr.length);
    while (diffPos < minLen && v[diffPos] === vr[diffPos]) diffPos++;
    const ctxStart = Math.max(0, diffPos - 40);
    const ctxEnd = diffPos + 60;

    examples[cat].push({
      file: m.file,
      vueCtx: v.slice(ctxStart, ctxEnd),
      verterCtx: vr.slice(ctxStart, Math.min(vr.length, ctxEnd)),
      diffPos
    });
  }
}

// Sort by count descending
const sorted = Object.entries(cats).sort((a,b) => b[1] - a[1]);
console.log('\n=== Mismatch Categories (deep) ===');
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
    console.log(`  File: ${ex.file} (diff at char ${ex.diffPos})`);
    console.log(`    Vue:    ...${ex.vueCtx}...`);
    console.log(`    Verter: ...${ex.verterCtx}...`);
  }
}
