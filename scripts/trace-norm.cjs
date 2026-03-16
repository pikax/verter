const path = require('path');
const fs = require('fs');
const { parse, compileScript, compileTemplate } = require('@vue/compiler-sfc');
const { VerterHost } = require(path.join(process.cwd(), 'packages/native/index.js'));

const filePath = 'D:/dev/github/verter-test-repos/ant-design-vue/components/form/demo/dynamic-form-item.vue';
const source = fs.readFileSync(filePath, 'utf-8');
const filename = path.basename(filePath);

const { descriptor } = parse(source, { filename });
let bindingMetadata = {};
try {
  const scriptResult = compileScript(descriptor, { id: filename, inlineTemplate: false });
  bindingMetadata = scriptResult.bindings || {};
} catch {}
const vueResult = compileTemplate({
  source: descriptor.template.content, filename, id: filename, ssr: true,
  compilerOptions: { mode: 'module', bindingMetadata }
});

const code = vueResult.code;
const fnIdx = code.indexOf('function ssrRender(');
const braceStart = code.indexOf('{', fnIdx);
let depth = 0, end = braceStart;
for (let i = braceStart; i < code.length; i++) {
  if (code[i] === '{') depth++;
  else if (code[i] === '}') { depth--; if (depth === 0) { end = i; break; } }
}
const body = code.slice(braceStart + 1, end).trim();

// Find the relevant mergeProps
const mpIdx = body.indexOf('_mergeProps({');
console.log('=== RAW Vue mergeProps ===');
console.log(body.slice(mpIdx, mpIdx + 600));
