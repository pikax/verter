// Performance breakdown: measure Rust codegen time vs tsc subprocess time
// by comparing verter-tsc total time against plain tsc on the same tsconfig.
import { spawnSync } from 'child_process';
import { existsSync, readFileSync, readdirSync } from 'fs';
import { join } from 'path';

const IS_WIN = process.platform === 'win32';
const REPOS = 'D:/dev/github/verter-test-repos';
const VERTER_TSC = 'D:/dev/personal/verter/target/release/verter-tsc.exe';

const PROJECTS = [
  { name: 'vuetify', tsconfig: 'tsconfig.json' },
  { name: 'element-plus', tsconfig: 'tsconfig.web.json' },
  { name: 'slidev', tsconfig: 'tsconfig.json' },
  { name: 'zyronon-douyin', tsconfig: 'tsconfig.app.json' },
];

function findTsc(root) {
  const cmd = join(root, 'node_modules/.bin', IS_WIN ? 'tsc.cmd' : 'tsc');
  return existsSync(cmd) ? cmd : null;
}

function findVueTsc(root) {
  const cmd = join(root, 'node_modules/.bin', IS_WIN ? 'vue-tsc.cmd' : 'vue-tsc');
  return existsSync(cmd) ? cmd : null;
}

function countVueFiles(dir) {
  let count = 0;
  const walk = (d) => {
    let entries;
    try { entries = readdirSync(d, { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      if (e.name === 'node_modules' || e.name.startsWith('.')) continue;
      if (e.isDirectory()) { walk(join(d, e.name)); continue; }
      if (e.name.endsWith('.vue')) count++;
    }
  };
  walk(dir);
  return count;
}

function run(bin, args, cwd) {
  const start = performance.now();
  const r = spawnSync(bin, args, {
    cwd, timeout: 5 * 60000, encoding: 'utf-8',
    shell: IS_WIN && (bin.endsWith('.cmd') || bin.endsWith('.bat')),
    windowsHide: true, env: { ...process.env, FORCE_COLOR: '0' },
  });
  const ms = performance.now() - start;
  const out = String(r.stdout || '') + String(r.stderr || '');
  const errs = (out.match(/error TS\d+:/g) || []).length;
  return { ms, exit: r.status || 0, errs };
}

function fmt(ms) { return ms < 1000 ? ms.toFixed(0) + 'ms' : (ms / 1000).toFixed(1) + 's'; }

const col = (s, w, right = true) => right ? String(s).padStart(w) : String(s).padEnd(w);

console.log('Performance Breakdown: plain tsc vs vue-tsc vs verter-tsc\n');
console.log('Each tool runs warm (2nd invocation). All use --noEmit --project <tsconfig>.\n');
console.log('  ' + [
  col('Project', 18, false),
  col('.vue', 5),
  col('plain-tsc', 10),
  col('vue-tsc', 10),
  col('verter-tsc', 11),
  col('overhead', 9),
  col('errs:tsc', 9),
  col('errs:vue', 9),
  col('errs:v', 7),
].join('  '));
console.log('  ' + '-'.repeat(88));

for (const p of PROJECTS) {
  const root = join(REPOS, p.name);
  if (!existsSync(join(root, 'node_modules'))) { console.log(`  ${p.name}: SKIP (no node_modules)`); continue; }

  const tsconfig = join(root, p.tsconfig);
  if (!existsSync(tsconfig)) { console.log(`  ${p.name}: SKIP (no ${p.tsconfig})`); continue; }

  const tsc = findTsc(root);
  const vueTsc = findVueTsc(root);
  const vueCount = countVueFiles(root);

  process.stderr.write(`  ${p.name}: `);

  // Plain tsc (warm)
  process.stderr.write('tsc...');
  if (tsc) {
    run(tsc, ['--noEmit', '--project', tsconfig], root); // cold
    var plainTsc = run(tsc, ['--noEmit', '--project', tsconfig], root); // warm
  }

  // vue-tsc (warm)
  process.stderr.write(' vue-tsc...');
  if (vueTsc) {
    run(vueTsc, ['--noEmit', '--project', tsconfig], root);
    var vueTscR = run(vueTsc, ['--noEmit', '--project', tsconfig], root);
  }

  // verter-tsc (warm)
  process.stderr.write(' verter-tsc...');
  run(VERTER_TSC, ['--noEmit', '--project', tsconfig], root);
  const verterTsc = run(VERTER_TSC, ['--noEmit', '--project', tsconfig], root);

  process.stderr.write(' done\n');

  // overhead = verter-tsc - plain-tsc (the cost of generating + processing .tsc.tsx)
  const overhead = plainTsc ? fmt(verterTsc.ms - plainTsc.ms) : '-';

  console.log('  ' + [
    col(p.name, 18, false),
    col(vueCount, 5),
    col(plainTsc ? fmt(plainTsc.ms) + (plainTsc.exit !== 0 ? '(e)' : '') : 'N/A', 10),
    col(vueTscR ? fmt(vueTscR.ms) + (vueTscR.exit !== 0 ? '(e)' : '') : 'N/A', 10),
    col(fmt(verterTsc.ms) + (verterTsc.exit !== 0 ? '(e)' : ''), 11),
    col(overhead, 9),
    col(plainTsc?.errs ?? '-', 9),
    col(vueTscR?.errs ?? '-', 9),
    col(verterTsc.errs, 7),
  ].join('  '));
}

console.log('  ' + '-'.repeat(88));
console.log('\n  overhead = verter-tsc - plain-tsc = cost of .tsc.tsx generation + Vue type instantiation');
console.log('  If overhead >> 0, the .tsc.tsx files add significant tsc work (import("vue") generic evaluation)');
console.log('  If verter-tsc < vue-tsc, verter wins. If verter-tsc > vue-tsc, Volar is more efficient.\n');
