/**
 * Apple-to-Apple Benchmark: @vue/compiler-sfc vs Verter vs Vize
 *
 * Runs all three compilers on identical input files with identical conditions:
 *   - Same 8 real-world Vue SFC fixtures (42B → 27KB)
 *   - Same machine, same process (single-threaded)
 *   - tinybench: 20 warmup + 100 measured iterations each
 *
 * Also runs a multi-threaded stress test (2000 files):
 *   - Vue: sequential (ST baseline)
 *   - Verter: compileBatch with Rayon (N threads = CPU count)
 *   - Vize: compileSfcBatch with Rayon (N threads = CPU count)
 *
 * Usage: node --import tsx src/apple-to-apple.ts
 */
import { Bench } from 'tinybench'
import { readFileSync, writeFileSync, mkdirSync, rmSync, existsSync, statSync } from 'node:fs'
import { join, dirname } from 'path'
import { fileURLToPath } from 'url'
import { availableParallelism, tmpdir } from 'os'
import { createRequire } from 'module'
import { compileVue } from './compilers/vue.js'
import { createVerterHost, compileVerterHost } from './compilers/verter.js'
import { compileBatch as verterCompileBatch } from '@verter/native'
import { formatDuration, formatBytes } from './utils/stats.js'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const _require = createRequire(__filename)
const CPU_COUNT = availableParallelism()

// ─── Load Vize native ────────────────────────────────────────────────────────
const VIZE_PATH = process.env.VIZE_PATH
if (!VIZE_PATH) {
  console.error('Set VIZE_PATH env var to the Vize repo root (e.g. /path/to/vize)')
  console.error('Then build: cd $VIZE_PATH && cargo build -p vize_vitrine --features napi')
  process.exit(1)
}
const VIZE_NATIVE_PATH = join(VIZE_PATH, 'npm/vize-native/index.js')
let vize: { compileSfc: (src: string, opts: { filename: string }) => { code: string }; compileSfcBatch: (pattern: string, opts?: { threads?: number }) => { success: number; failed: number; timeMs: number; inputBytes: number; outputBytes: number } } | null = null
try {
  vize = _require(VIZE_NATIVE_PATH)
  console.log('Vize native loaded')
} catch (e: any) {
  console.error(`Could not load Vize native from ${VIZE_NATIVE_PATH}`)
  console.error(`Build it with: cd ${VIZE_PATH} && cargo build -p vize_vitrine --features napi`)
  process.exit(1)
}

// verterCompileBatch is imported at the top of the file from '@verter/native'
console.log('✅ Verter native loaded')

// ─── Fixtures ────────────────────────────────────────────────────────────────
const FIXTURE_NAMES = [
  'tiny-template.vue',
  'simple-interactive.vue',
  'list-rendering.vue',
  'conditional-heavy.vue',
  'form-component.vue',
  'composition-heavy.vue',
  'template-heavy.vue',
  'kitchen-sink.vue',
]

interface Fixture { name: string; filename: string; source: string; size: number }

const FIXTURES_DIR = join(__dirname, 'fixtures')
const fixtures: Fixture[] = FIXTURE_NAMES.map(filename => {
  const source = readFileSync(join(FIXTURES_DIR, filename), 'utf-8')
  return { name: filename.replace('.vue', ''), filename, source, size: Buffer.byteLength(source, 'utf-8') }
})

// ─── Detect Vize build type ─────────────────────────────────────────────────
const vizeReleasePath = join(VIZE_PATH, 'target/release/vize_vitrine.dll')
const vizeNodePath = join(VIZE_PATH, 'npm/vize-native/vize-vitrine.win32-x64-msvc.node')
function detectVizeBuildType(): 'release' | 'debug' {
  try {
    if (!existsSync(vizeReleasePath) || !existsSync(vizeNodePath)) return 'debug'
    const releaseMtime = statSync(vizeReleasePath).mtimeMs
    const nodeMtime = statSync(vizeNodePath).mtimeMs
    // If the .node file was modified within 5 seconds of the release DLL, it's a release build
    return Math.abs(nodeMtime - releaseMtime) < 5000 ? 'release' : 'debug'
  } catch { return 'debug' }
}
const vizeBuildType = detectVizeBuildType()

// ─── Print header ────────────────────────────────────────────────────────────
console.log('\n' + '═'.repeat(74))
console.log(' 🔬 Apple-to-Apple Benchmark: @vue/compiler-sfc vs Verter vs Vize')
console.log('═'.repeat(74))
console.log(`\n  CPU cores  : ${CPU_COUNT}`)
console.log(`  Vize build : ${vizeBuildType.toUpperCase()}${vizeBuildType === 'debug' ? ' (unoptimized — release build would be faster)' : ' (optimized)'}`)
console.log(`  Verter     : release build (workspace)`)
console.log(`  Vue        : @vue/compiler-sfc JS (single-threaded)`)
console.log(`  Warmup     : 20 iterations | Measured: 100 iterations per fixture`)

// ─── SECTION 1: Single-Threaded ──────────────────────────────────────────────
console.log('\n' + '─'.repeat(74))
console.log(' § 1  Single-Threaded: All 3 compilers, same fixtures, same thread')
console.log('─'.repeat(74))

const col = (s: string | number, w: number, right = true) =>
  right ? String(s).padStart(w) : String(s).padEnd(w)

console.log(`\n  ${col('Fixture', 24, false)}${col('Size', 8)}  ${col('Vue', 10)}  ${col('Verter', 10)}  ${col('Vize', 10)}  ${col('Verter/Vue', 11)}  ${col('Vize/Vue', 9)}`)
console.log('  ' + '─'.repeat(88))

interface STResult { name: string; size: number; vueMean: number; verterMean: number; vizeMean: number }
const stResults: STResult[] = []

for (const f of fixtures) {
  const host = createVerterHost('none')
  const bench = new Bench({ warmupIterations: 20, iterations: 100 })

  bench.add('vue',    () => { compileVue(f.source, f.filename) })
  bench.add('verter', () => { compileVerterHost(host, f.source, f.filename) })
  bench.add('vize',   () => { vize!.compileSfc(f.source, { filename: f.filename }) })

  await bench.run()

  const vue    = (bench.getTask('vue')!.result! as any).latency?.mean || 0
  const verter = (bench.getTask('verter')!.result! as any).latency?.mean || 0
  const vizeMs = (bench.getTask('vize')!.result! as any).latency?.mean || 0

  stResults.push({ name: f.name, size: f.size, vueMean: vue, verterMean: verter, vizeMean: vizeMs })

  const vSpeedup = (vue / verter).toFixed(1) + 'x'
  const zSpeedup = (vue / vizeMs).toFixed(1) + 'x'
  console.log(`  ${col(f.name, 24, false)}${col(formatBytes(f.size), 8)}  ${col(formatDuration(vue), 10)}  ${col(formatDuration(verter), 10)}  ${col(formatDuration(vizeMs), 10)}  ${col(vSpeedup, 11)}  ${col(zSpeedup, 9)}`)
}

const avgVerterSpeedup = stResults.reduce((s, r) => s + r.vueMean / r.verterMean, 0) / stResults.length
const avgVizeSpeedup   = stResults.reduce((s, r) => s + r.vueMean / r.vizeMean, 0) / stResults.length

console.log('  ' + '─'.repeat(88))
console.log(`  ${col('AVERAGE', 24, false)}${col('', 8)}  ${col('', 10)}  ${col('', 10)}  ${col('', 10)}  ${col(avgVerterSpeedup.toFixed(1) + 'x', 11)}  ${col(avgVizeSpeedup.toFixed(1) + 'x', 9)}`)
console.log()

// ─── SECTION 2: Multi-Threaded Stress Test ───────────────────────────────────
console.log('─'.repeat(74))
console.log(` § 2  Multi-Threaded Stress Test — 2000 files (8 fixtures × 250), ${CPU_COUNT} cores`)
console.log('─'.repeat(74))

// Create 2000 temp files (needed for Vize glob API)
const TEMP_DIR = join(tmpdir(), 'apple-bench-' + Date.now())
mkdirSync(TEMP_DIR, { recursive: true })
const allFiles: Array<{ filename: string; source: string }> = []
for (let i = 0; i < 250; i++) {
  for (let fi = 0; fi < fixtures.length; fi++) {
    const f = fixtures[fi]
    const filename = `${String(i * 8 + fi).padStart(4, '0')}-${f.filename}`
    writeFileSync(join(TEMP_DIR, filename), f.source)
    allFiles.push({ filename, source: f.source })
  }
}
const totalBytes = allFiles.reduce((s, f) => s + Buffer.byteLength(f.source, 'utf-8'), 0)
console.log(`\n  Written ${allFiles.length} files (${formatBytes(totalBytes)}) to temp dir`)

// 2a. Vue ST baseline (sequential)
process.stdout.write('  Running Vue (ST baseline)...')
const vueSTStart = performance.now()
for (const f of allFiles) { compileVue(f.source, f.filename) }
const vueSTMs = performance.now() - vueSTStart
console.log(` done — ${formatDuration(vueSTMs)}`)

// 2b. Verter MT via compileBatch (native Rayon parallelism)
process.stdout.write(`  Running Verter MT (${CPU_COUNT} Rayon threads)...`)
const verterMTStart = performance.now()
const verterBatch = verterCompileBatch(allFiles, { threads: CPU_COUNT })
const verterMTMs = performance.now() - verterMTStart
const verterSucceeded = verterBatch.filter(r => !r.error).length
console.log(` done — ${formatDuration(verterMTMs)} (${verterSucceeded} succeeded)`)

// 2c. Vize MT via compileSfcBatch (native Rayon parallelism)
process.stdout.write(`  Running Vize MT (${CPU_COUNT} Rayon threads)...`)
const vizeGlob = join(TEMP_DIR, '*.vue').replace(/\\/g, '/')
const vizeBatch = vize!.compileSfcBatch(vizeGlob, { threads: CPU_COUNT })
const vizeMTMs = vizeBatch.timeMs
console.log(` done — ${formatDuration(vizeMTMs)} (${vizeBatch.success} succeeded)`)

// Print MT results
console.log(`\n  ${col('Compiler', 22, false)}  ${col('Config', 17, false)}  ${col('Time', 10)}  ${col('Files/s', 10)}  ${col('vs Vue ST', 10)}`)
console.log('  ' + '─'.repeat(76))

const fmtRow = (name: string, config: string, ms: number, baseline: number) => {
  const fps = Math.round((allFiles.length / ms) * 1000)
  const speedup = ms > 0 ? (baseline / ms).toFixed(1) + 'x' : 'N/A'
  console.log(`  ${col(name, 22, false)}  ${col(config, 17, false)}  ${col(formatDuration(ms), 10)}  ${col(fps.toLocaleString(), 10)}  ${col(speedup, 10)}`)
}

fmtRow('@vue/compiler-sfc', `ST (1 thread)`, vueSTMs, vueSTMs)
fmtRow('Verter', `MT (${CPU_COUNT} Rayon threads)`, verterMTMs, vueSTMs)
fmtRow('Vize', `MT (${CPU_COUNT} Rayon threads)`, vizeMTMs, vueSTMs)
console.log()

// Cleanup
try { rmSync(TEMP_DIR, { recursive: true, force: true }) } catch {}

// ─── Footer ──────────────────────────────────────────────────────────────────
console.log('═'.repeat(74))
if (vizeBuildType === 'debug') {
  console.log(' ⚠️  BUILD NOTE: Vize is using a DEBUG build (no optimizations).')
  console.log('     For accurate results, rebuild with:')
  console.log(`       cd ${VIZE_PATH} && cargo build -p vize_vitrine --features napi --release`)
  console.log('       cp target/release/vize_vitrine.dll npm/vize-native/vize-vitrine.win32-x64-msvc.node')
} else {
  console.log(' ✅ Both Vize and Verter are using RELEASE builds (optimized). Results are accurate.')
}
console.log(' Verter MT: compileBatch with Rayon (native parallelism, source in memory).')
console.log(' Vize MT: compileSfcBatch with Rayon (native parallelism, reads from disk).')
console.log('═'.repeat(74))
console.log()
