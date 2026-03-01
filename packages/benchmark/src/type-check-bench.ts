/**
 * Type-Check Benchmark: vue-tsc vs verter-tsc
 *
 * Measures wall-clock type-checking time on real-world Vue projects.
 * Uses spawnSync (not tinybench) since type-checking takes seconds, not microseconds.
 *
 * Tools:
 *   vue-tsc    — project node_modules/.bin/vue-tsc (Volar-based)
 *   verter-tsc — our Rust binary (macro-only .tsc.tsx → tsc subprocess)
 *
 * Both tools run: --noEmit --project <tsconfig>
 *
 * Usage:
 *   node --import tsx src/type-check-bench.ts
 *   node --import tsx src/type-check-bench.ts --errors   # show error counts
 *   node --import tsx src/type-check-bench.ts --project slidev  # single project
 */

import { spawnSync } from 'node:child_process'
import { existsSync, statSync, readdirSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

// ─── CLI flags ────────────────────────────────────────────────────────────────
const SHOW_ERRORS = process.argv.includes('--errors')
const FILTER_PROJECT = (() => {
  const i = process.argv.indexOf('--project')
  return i !== -1 ? process.argv[i + 1] : null
})()
const TIMEOUT_MS = 5 * 60 * 1000 // 5 minutes per run

// ─── Platform helpers ─────────────────────────────────────────────────────────
const IS_WIN = process.platform === 'win32'
const EXE = IS_WIN ? '.exe' : ''

// ─── Binary paths ─────────────────────────────────────────────────────────────
const VERTER_ROOT = resolve(__dirname, '..', '..', '..')
const VERTER_RELEASE = join(VERTER_ROOT, 'target', 'release', `verter-tsc${EXE}`)
const VERTER_DEBUG   = join(VERTER_ROOT, 'target', 'debug',   `verter-tsc${EXE}`)

// ─── Build type detection ─────────────────────────────────────────────────────
function detectBuildType(release: string, debug: string): { bin: string | null; type: 'release' | 'debug' | 'missing' } {
  const hasRelease = existsSync(release)
  const hasDebug   = existsSync(debug)
  if (!hasRelease && !hasDebug) return { bin: null, type: 'missing' }
  if (!hasRelease) return { bin: debug,   type: 'debug' }
  if (!hasDebug)   return { bin: release, type: 'release' }
  // Both exist — pick newer
  const relMtime = statSync(release).mtimeMs
  const dbgMtime = statSync(debug).mtimeMs
  return relMtime >= dbgMtime
    ? { bin: release, type: 'release' }
    : { bin: debug,   type: 'debug' }
}

const verter = detectBuildType(VERTER_RELEASE, VERTER_DEBUG)

// ─── vue-tsc discovery ────────────────────────────────────────────────────────
// Returns the vue-tsc invocation args. Prefers project-local, falls back to npx.
function findVueTsc(projectRoot: string): { bin: string; args: string[] } {
  const binDir = join(projectRoot, 'node_modules', '.bin')
  const cmd = join(binDir, IS_WIN ? 'vue-tsc.cmd' : 'vue-tsc')
  if (existsSync(cmd)) return { bin: cmd, args: [] }
  const plain = join(binDir, 'vue-tsc')
  if (existsSync(plain)) return { bin: plain, args: [] }
  // Fall back to npx (uses globally cached version).
  return { bin: IS_WIN ? 'npx.cmd' : 'npx', args: ['vue-tsc'] }
}

// ─── Projects ─────────────────────────────────────────────────────────────────
const TEST_REPOS = 'D:/dev/github/verter-test-repos'

interface Project {
  name: string
  root: string
  tsconfig: string
  note?: string
}

const PROJECTS: Project[] = [
  {
    name: 'slidev',
    root: join(TEST_REPOS, 'slidev'),
    tsconfig: join(TEST_REPOS, 'slidev', 'tsconfig.json'),
  },
  {
    // element-plus root tsconfig uses project references; tsconfig.web.json has the Vue packages
    name: 'element-plus',
    root: join(TEST_REPOS, 'element-plus'),
    tsconfig: join(TEST_REPOS, 'element-plus', 'tsconfig.web.json'),
  },
  {
    name: 'nuxt-ui',
    root: join(TEST_REPOS, 'nuxt-ui'),
    tsconfig: join(TEST_REPOS, 'nuxt-ui', 'tsconfig.json'),
    note: 'needs .nuxt/ generated',
  },
]

// ─── .vue file counter ────────────────────────────────────────────────────────
function countVueFiles(dir: string): number {
  let count = 0
  const walk = (d: string) => {
    let entries: import('node:fs').Dirent<string>[]
    try { entries = readdirSync(d, { withFileTypes: true, encoding: 'utf-8' }) } catch { return }
    for (const e of entries) {
      if (e.name === 'node_modules' || e.name.startsWith('.')) continue
      if (e.isDirectory()) { walk(join(d, e.name)); continue }
      if (e.name.endsWith('.vue')) count++
    }
  }
  walk(dir)
  return count
}

// ─── Run a tool ───────────────────────────────────────────────────────────────
interface RunResult {
  ms: number
  exitCode: number
  errorCount: number
  timedOut: boolean
  skipped: boolean
  skipReason?: string
}

function runTool(bin: string, args: string[], cwd: string): RunResult {
  const start = performance.now()
  const r = spawnSync(bin, args, {
    cwd,
    timeout: TIMEOUT_MS,
    encoding: 'utf-8',
    shell: IS_WIN && (bin.endsWith('.cmd') || bin.endsWith('.bat')),
    windowsHide: true,
  })
  const ms = performance.now() - start

  if (r.error?.message?.includes('ETIMEDOUT') || r.signal === 'SIGTERM') {
    return { ms, exitCode: -1, errorCount: 0, timedOut: true, skipped: false }
  }

  const out = String(r.stdout ?? '') + String(r.stderr ?? '')
  const errorCount = (out.match(/error TS\d+:/g) ?? []).length
  return { ms, exitCode: r.status ?? -1, errorCount, timedOut: false, skipped: false }
}

function skipped(reason: string): RunResult {
  return { ms: 0, exitCode: 0, errorCount: 0, timedOut: false, skipped: true, skipReason: reason }
}

// ─── Column formatter ─────────────────────────────────────────────────────────
const col = (s: string | number, w: number, right = true) =>
  right ? String(s).padStart(w) : String(s).padEnd(w)

function fmtMs(ms: number): string {
  if (ms < 1000) return `${ms.toFixed(0)}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

function fmtResult(r: RunResult, baseline?: RunResult): { time: string; speedup: string } {
  if (r.skipped)   return { time: col('N/A', 9),       speedup: col('', 7) }
  if (r.timedOut)  return { time: col('>5min', 9),     speedup: col('', 7) }
  const timeStr = fmtMs(r.ms) + (r.exitCode !== 0 ? '(err)' : '')
  const speedup = baseline && !baseline.skipped && !baseline.timedOut && r.ms > 0
    ? (baseline.ms / r.ms).toFixed(1) + 'x'
    : ''
  return { time: col(timeStr, 9), speedup: col(speedup, 7) }
}

// ─── Header ───────────────────────────────────────────────────────────────────
const W = 80
console.log('\n' + '='.repeat(W))
console.log(' Type-Check Benchmark: vue-tsc vs verter-tsc')
console.log('='.repeat(W))
console.log()
console.log(`  verter-tsc : ${verter.type.toUpperCase().padEnd(7)} (${verter.bin ?? 'NOT FOUND'})`)
console.log(`  vue-tsc    : project node_modules/.bin/vue-tsc, fallback to npx vue-tsc`)
console.log(`  Timeout    : 5 min per run`)
if (SHOW_ERRORS) console.log(`  Mode       : showing error counts`)
if (verter.type === 'debug') console.log(`\n  !!  verter-tsc is a DEBUG build — run: cargo build --package verter_tsc --release`)

// ─── Table header ─────────────────────────────────────────────────────────────
console.log()
const hdr = [
  col('Project',     20, false),
  col('.vue', 5),
  col('vue-tsc',  9),
  col('v-tsc',    9),
  col('speedup',  7),
]
if (SHOW_ERRORS) hdr.push(col('errs:vue', 9), col('errs:v',  8))
console.log('  ' + hdr.join('  '))
console.log('  ' + '-'.repeat(W - 2))

// ─── Run benchmarks ───────────────────────────────────────────────────────────
interface RowResult {
  name: string
  vueFiles: number
  vueTsc: RunResult
  verterTsc: RunResult
}

const results: RowResult[] = []

for (const proj of PROJECTS) {
  if (FILTER_PROJECT && proj.name !== FILTER_PROJECT) continue

  const exists = existsSync(proj.tsconfig)
  if (!exists) {
    process.stdout.write(`  ${col(proj.name, 20, false)}  (skipped — tsconfig not found)\n`)
    continue
  }

  // Warn about nuxt-ui needing generated types
  if (proj.note) {
    const nuxtTsconfig = join(proj.root, '.nuxt', 'tsconfig.json')
    if (!existsSync(nuxtTsconfig)) {
      process.stdout.write(`  ${col(proj.name, 20, false)}  !!  .nuxt/ not generated — run 'nuxi prepare' first\n`)
    }
  }

  const vueFiles = countVueFiles(proj.root)
  process.stderr.write(`  [${proj.name}] vue-tsc...`)

  // vue-tsc: --noEmit --project <tsconfig>
  const vueTscInfo = findVueTsc(proj.root)
  const vueTsc = runTool(vueTscInfo.bin, [...vueTscInfo.args, '--noEmit', '--project', proj.tsconfig], proj.root)
  process.stderr.write(` verter-tsc...`)

  // verter-tsc: --noEmit --project <tsconfig>
  const verterTsc = verter.bin
    ? runTool(verter.bin, ['--noEmit', '--project', proj.tsconfig], proj.root)
    : skipped('binary not found')
  process.stderr.write(` done\n`)

  const { time: vueTscTime }    = fmtResult(vueTsc)
  const { time: verterTime, speedup: verterSpeedup } = fmtResult(verterTsc, vueTsc)

  const row = [
    col(proj.name, 20, false),
    col(vueFiles, 5),
    vueTscTime,
    verterTime,
    verterSpeedup,
  ]
  if (SHOW_ERRORS) {
    row.push(
      col(vueTsc.skipped    ? '-' : String(vueTsc.errorCount),    9),
      col(verterTsc.skipped ? '-' : String(verterTsc.errorCount), 8),
    )
  }
  console.log('  ' + row.join('  '))

  results.push({ name: proj.name, vueFiles, vueTsc, verterTsc })
}

// ─── Footer ───────────────────────────────────────────────────────────────────
console.log('  ' + '-'.repeat(W - 2))
console.log()
console.log('='.repeat(W))

const warnings: string[] = []
if (verter.type === 'debug') warnings.push('verter-tsc: DEBUG build — cargo build --package verter_tsc --release')

if (warnings.length > 0) {
  console.log()
  for (const w of warnings) console.log(` !!  ${w}`)
}

console.log()
console.log(' Both tools run: --noEmit --project <tsconfig>')
console.log('   verter-tsc: macro-only .tsc.tsx generation + tsc subprocess')
console.log('   vue-tsc:    full Volar language plugin + tsc')
console.log('='.repeat(W))
console.log()
