const path = require('path');
const fs = require('fs');
const { parse, compileScript, compileTemplate } = require('@vue/compiler-sfc');
const d = JSON.parse(fs.readFileSync('C:/temp/ssr-iter-114.json', 'utf8'));

const bp = s => s.replace(/\$setup\./g, '_ctx.').replace(/\$setup\["/g, '_ctx["').replace(/\$props\./g, '_ctx.').replace(/\$props\["/g, '_ctx["');

let count = 0;
for (const r of d.mismatches) {
  const v = r.vue || '', t = r.verter || '';
  if (bp(v) !== bp(t)) continue;

  const fullPath = 'D:/dev/' + r.file;
  let source;
  try {
    source = fs.readFileSync(fullPath, 'utf8');
  } catch { continue; }

  const { descriptor } = parse(source);
  let bindingMetadata = {};
  let scriptError = null;

  if (descriptor.script || descriptor.scriptSetup) {
    try {
      const scriptResult = compileScript(descriptor, {
        id: fullPath,
        inlineTemplate: false,
      });
      bindingMetadata = scriptResult.bindings || {};
    } catch (e) {
      scriptError = e.message;
    }
  }

  const bindings = Object.entries(bindingMetadata)
    .filter(([k, v]) => ['setup-const', 'setup-let', 'setup-maybe-ref', 'setup-reactive-const', 'setup-ref', 'props', 'props-aliased'].includes(v))
    .slice(0, 8);

  count++;
  console.log(`${r.file.split('/').slice(-2).join('/')}: ${scriptError ? 'SCRIPT_ERROR: ' + scriptError.slice(0, 80) : 'bindings=' + JSON.stringify(Object.fromEntries(bindings)).slice(0, 120)}`);

  if (count >= 22) break;
}
