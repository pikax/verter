const { normalizeForComparison, extractSsrRenderBody } = require('./scripts/ssr-baseline/normalize.mjs');

// Simulate the raw Vue mergeProps section
const raw = `_mergeProps({
    key: domain.key
  }, { ref_for: true }, index === 0 ? $setup.formItemLayout : {}, {
    label: index === 0 ? 'Domains' : '',
    name: ['domains', index, 'value'],
    rules: {
      required: true,
      message: 'domain can not be null',
      trigger: 'change',
    }
  })`;

console.log('=== Raw ===');
console.log(raw);

// Now normalize just this piece
const result = normalizeForComparison(raw);
console.log('\n=== Normalized ===');
console.log(result);

console.log('\nHas label:', result.includes('label'));
console.log('Has rules:', result.includes('rules'));
console.log('Has Domains:', result.includes('Domains'));
console.log('Has _mergeProps:', result.includes('_mergeProps'));
