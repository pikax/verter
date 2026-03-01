import fs from 'fs';

const d = JSON.parse(fs.readFileSync('C:/temp/ssr-full-vmodel-merge.json', 'utf8'));
const mm = d.mismatches;

// Count casing mismatches across ALL categories
let casingCount = 0;
const casingByProject = {};

for (const m of mm) {
  const vueLines = (m.vue || '').split('\n');
  const verterLines = (m.verter || '').split('\n');
  const maxLen = Math.max(vueLines.length, verterLines.length);

  let hasCasing = false;
  for (let i = 0; i < maxLen; i++) {
    const vl = (vueLines[i] || '').trim();
    const vrl = (verterLines[i] || '').trim();
    if (vl !== vrl) {
      // Check if the diff is a casing difference
      if (vl.toLowerCase() === vrl.toLowerCase()) {
        hasCasing = true;
        break;
      }
      // Check character-by-character for casing at diff point
      let dp = 0;
      const ml = Math.min(vl.length, vrl.length);
      while (dp < ml && vl[dp] === vrl[dp]) dp++;
      if (dp < ml) {
        const vc = vl[dp], vrc = vrl[dp];
        if (vc && vrc && vc.toLowerCase() === vrc.toLowerCase()) {
          hasCasing = true;
          break;
        }
      }
    }
  }

  if (hasCasing) {
    casingCount++;
    // Extract project name
    const parts = m.file.split('/');
    const projIdx = parts.indexOf('verter-test-repos');
    const proj = projIdx >= 0 ? parts[projIdx + 1] : 'other';
    casingByProject[proj] = (casingByProject[proj] || 0) + 1;
  }
}

console.log('Total casing mismatches:', casingCount);
console.log('\nBy project:');
const sorted = Object.entries(casingByProject).sort((a,b) => b[1] - a[1]);
for (const [proj, count] of sorted) {
  console.log(`  ${proj}: ${count}`);
}
