import fs from 'fs';
import path from 'path';
import { execSync } from 'child_process';

const pkgRoot = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..');
const benchDir = path.join(pkgRoot, 'bench');
const genDir = path.join(benchDir, 'generated');
const tsconfigBench = path.join(pkgRoot, 'tsconfig.bench.json');

function ensureDirs() {
  if (!fs.existsSync(genDir)) fs.mkdirSync(genDir, { recursive: true });
}

function genBenchFile(size) {
  const file = path.join(genDir, `bench-size-${size}.ts`);
  // Generate a large object type with many keys
  const props = Array.from({ length: size }, (_, i) => `  P${i + 1}: number | undefined;`).join('\n');
  // Generate a union with many types
  const unionCount = Math.min(size, 40);
  const unionParts = Array.from({ length: unionCount }, (_, i) => `{ K${i + 1}: ${i + 1} }`).join(' | ');
  // Generate many event signatures for IntersectionFunctionToObject
  const evtCount = Math.min(size, 20);
  const eventSigs = Array.from({ length: evtCount }, (_, i) => `((e: '${`evt${i + 1}`}', ...args: [number, string]) => void)`).join(' & ');

  const content = `
import type { PartialUndefined, UnionToIntersection, FunctionToObject, IntersectionFunctionToObject } from '../../src/helpers/helpers.ts';

type LargeObj_${size} = {
${props}
};

// Force evaluation of mapped + conditional type
type R1_${size} = PartialUndefined<LargeObj_${size}>;
const _r1_${size}: R1_${size} | null = null as any;

// Force evaluation of deep conditional instantiation
type U_${size} = ${unionParts};
type R2_${size} = UnionToIntersection<U_${size}>;
const _r2_${size}: R2_${size} | null = null as any;

// Force evaluation of intersection of function overloads
type EvtFns_${size} = ${eventSigs};
type R3_${size} = IntersectionFunctionToObject<EvtFns_${size}>;
const _r3_${size}: R3_${size} | null = null as any;
`;

  fs.writeFileSync(file, content);
  return file;
}

function runTsc({ traceDir } = {}) {
  const args = [
    'pnpm', 'exec', 'tsc',
    '-p', tsconfigBench,
    '--extendedDiagnostics',
    '--pretty', 'false',
  ];
  if (traceDir) {
    args.push('--generateTrace', traceDir);
  }
  const cmd = args.join(' ');
  const out = execSync(cmd, { cwd: pkgRoot, stdio: 'pipe', encoding: 'utf8' });
  return out;
}

function parseDiagnostics(output) {
  const getNumber = (re) => {
    const m = output.match(re);
    return m ? parseFloat(m[1]) : NaN;
  };
  // Memory can be reported in MB or K; normalize to MB
  let memoryMB = NaN;
  const mMB = output.match(/Memory used:\s+(\d+\.?\d*)MB/);
  const mK = output.match(/Memory used:\s+(\d+\.?\d*)K/);
  if (mMB) memoryMB = parseFloat(mMB[1]);
  else if (mK) memoryMB = parseFloat(mK[1]) / 1024;

  return {
    files: getNumber(/Files:\s+(\d+)/),
    lines: getNumber(/Lines:\s+(\d+)/),
    nodes: getNumber(/Nodes:\s+(\d+)/),
    identifiers: getNumber(/Identifiers:\s+(\d+)/),
    symbols: getNumber(/Symbols:\s+(\d+)/),
    types: getNumber(/Types:\s+(\d+)/),
    memoryMB,
    parseMs: getNumber(/Parse time:\s+(\d+\.?\d*)s/)*1000,
    bindMs: getNumber(/Bind time:\s+(\d+\.?\d*)s/)*1000,
    checkMs: getNumber(/Check time:\s+(\d+\.?\d*)s/)*1000,
    emitMs: getNumber(/Emit time:\s+(\d+\.?\d*)s/)*1000,
    totalMs: getNumber(/Total time:\s+(\d+\.?\d*)s/)*1000,
  };
}

function main() {
  ensureDirs();
  const args = process.argv.slice(2);
  const trace = args.includes('--trace');
  const sizesArg = args.find(a => a.startsWith('--sizes='));
  const sizes = sizesArg ? sizesArg.replace('--sizes=','').split(',').map(s=>parseInt(s,10)) : [10, 50, 100, 200, 500];

  const results = [];
  for (const size of sizes) {
    genBenchFile(size);
    const traceDir = trace ? path.join(benchDir, `trace-${size}`) : undefined;
    if (traceDir && !fs.existsSync(traceDir)) fs.mkdirSync(traceDir, { recursive: true });
    let out = '';
    let failed = false;
    try {
      out = runTsc({ traceDir });
    } catch (e) {
      failed = true;
      out = (e && e.stdout) ? String(e.stdout) : String(e);
    }
    const stats = parseDiagnostics(out);
    results.push({ size, failed, ...stats });
    const prefix = failed ? '✗' : '✓';
    console.log(`${prefix} size=${size} total=${(stats.totalMs||0).toFixed(0)}ms check=${(stats.checkMs||0).toFixed(0)}ms mem=${(stats.memoryMB||0).toFixed(1)}MB types=${stats.types}`);
  }

  // Print a Markdown table
  console.log('\nBenchmark results (TypeScript extendedDiagnostics)');
  console.log('| Size | Status | Total ms | Check ms | Memory MB | Types | Nodes |');
  console.log('|-----:|:------:|---------:|---------:|----------:|------:|------:|');
  for (const r of results) {
    console.log(`| ${r.size} | ${r.failed ? 'fail' : 'ok'} | ${(r.totalMs||0).toFixed(0)} | ${(r.checkMs||0).toFixed(0)} | ${(r.memoryMB||0).toFixed(1)} | ${r.types||''} | ${r.nodes||''} |`);
  }
}

main();
