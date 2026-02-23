import { Bench } from 'tinybench'
import { readFileSync } from 'fs'
import { join, dirname } from 'path'
import { fileURLToPath } from 'url'
import { compileVue } from './compilers/vue'
import { createVerterHost, compileVerterHost } from './compilers/verter'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

const FIXTURES = [
  'tiny-template.vue',
  'simple-interactive.vue',
  'list-rendering.vue',
  'conditional-heavy.vue',
  'form-component.vue',
  'composition-heavy.vue',
  'template-heavy.vue',
  'kitchen-sink.vue'
]

interface Fixture {
  name: string
  source: string
  size: number
}

interface CompilerResult {
  mean: number
  opsPerSec: number
  errors: string[]
}

interface FixtureResults {
  name: string
  size: number
  vue: CompilerResult
  hostFull: CompilerResult
  hostEssential: CompilerResult
  hostNone: CompilerResult
}

function loadFixtures(): Fixture[] {
  const fixturesDir = join(__dirname, 'fixtures')
  return FIXTURES.map(filename => {
    const filepath = join(fixturesDir, filename)
    const source = readFileSync(filepath, 'utf-8')
    return {
      name: filename.replace('.vue', ''),
      source,
      size: Buffer.byteLength(source, 'utf-8')
    }
  })
}

async function benchSync(
  label: string,
  fn: () => { errors: string[] },
  iterations: number = 200
): Promise<CompilerResult> {
  const errors: string[] = []

  const bench = new Bench({
    time: 1000,
    warmupIterations: 10,
    iterations
  })

  bench.add(label, () => {
    const result = fn()
    if (result.errors.length > 0 && errors.length === 0) {
      errors.push(...result.errors.slice(0, 3))
    }
  })

  await bench.run()
  const task = bench.tasks[0]!

  return {
    mean: task.result?.mean || 0,
    opsPerSec: task.result?.hz || 0,
    errors
  }
}

function speedupStr(baseMs: number, compMs: number): string {
  if (compMs === 0) return ''
  const factor = baseMs / compMs
  const pct = ((1 - compMs / baseMs) * 100).toFixed(0)
  if (factor >= 1) {
    return `\x1b[32m${factor.toFixed(2)}x faster (${pct}%)\x1b[0m`
  } else {
    return `\x1b[31m${(1 / factor).toFixed(2)}x slower (${Math.abs(Number(pct))}%)\x1b[0m`
  }
}

async function benchFixture(fixture: Fixture): Promise<FixtureResults> {
  const sizeKB = (fixture.size / 1024).toFixed(2)
  console.log(`\n  ${fixture.name} (${sizeKB} KB)`)

  const fname = `${fixture.name}.vue`

  // Vue
  const vue = await benchSync('vue', () => compileVue(fixture.source, fname))

  // VerterHost with AnalysisLevel::Full (OXC + lightningcss)
  const hFull = createVerterHost('full')
  const hostFull = await benchSync('host-full', () => {
    return compileVerterHost(hFull, fixture.source, fname)
  })

  // VerterHost with AnalysisLevel::Essential (OXC only)
  const hEssential = createVerterHost('essential')
  const hostEssential = await benchSync('host-essential', () => {
    return compileVerterHost(hEssential, fixture.source, fname)
  })

  // VerterHost with AnalysisLevel::None (no extra analysis)
  const hNone = createVerterHost('none')
  const hostNone = await benchSync('host-none', () => {
    return compileVerterHost(hNone, fixture.source, fname)
  })

  // Print row
  const pad = (s: string, n: number) => s.padStart(n)
  const fmtLine = (label: string, r: CompilerResult, showSpeedup = true) => {
    const line = `    ${label.padEnd(18)} ${pad(r.mean.toFixed(3), 8)} ms  (${pad(r.opsPerSec.toFixed(0), 6)} ops/s)`
    const speedup = showSpeedup ? `  ${speedupStr(vue.mean, r.mean)}` : ''
    const errs = r.errors.length ? ` [${r.errors.length} errors]` : ''
    return line + speedup + errs
  }

  console.log(fmtLine('Vue:', vue, false))
  console.log(fmtLine('Host (full):', hostFull))
  console.log(fmtLine('Host (essential):', hostEssential))
  console.log(fmtLine('Host (none):', hostNone))

  return {
    name: fixture.name,
    size: fixture.size,
    vue,
    hostFull,
    hostEssential,
    hostNone
  }
}

async function stressTest(fixtures: Fixture[]) {
  const targetFiles = 20000
  const timesPerFixture = Math.ceil(targetFiles / fixtures.length)
  const allSources: { source: string; name: string }[] = []
  for (let i = 0; i < timesPerFixture; i++) {
    for (const f of fixtures) {
      allSources.push({ source: f.source, name: `${f.name}-${i}` })
    }
  }
  const totalSize = allSources.reduce((sum, s) => sum + Buffer.byteLength(s.source, 'utf-8'), 0)
  const totalSizeMB = (totalSize / (1024 * 1024)).toFixed(2)
  console.log(`\n  stress-test: ${allSources.length} files (${totalSizeMB} MB total)`)

  const run = async (label: string, fn: () => void) => {
    const bench = new Bench({ time: 30000, warmupIterations: 0, iterations: 1 })
    bench.add(label, fn)
    await bench.run()
    return bench.tasks[0]!.result?.mean || 0
  }

  const vueMean = await run('vue', () => {
    for (const s of allSources) compileVue(s.source, `${s.name}.vue`)
  })

  const hostFullMean = await run('host-full', () => {
    const host = createVerterHost('full')
    for (const s of allSources) compileVerterHost(host, s.source, `${s.name}.vue`)
  })

  const hostEssentialMean = await run('host-essential', () => {
    const host = createVerterHost('essential')
    for (const s of allSources) compileVerterHost(host, s.source, `${s.name}.vue`)
  })

  const hostNoneMean = await run('host-none', () => {
    const host = createVerterHost('none')
    for (const s of allSources) compileVerterHost(host, s.source, `${s.name}.vue`)
  })

  const pad = (s: string, n: number) => s.padStart(n)
  const msPerFile = (ms: number) => (ms / allSources.length).toFixed(3)
  console.log(`    Vue:              ${pad(vueMean.toFixed(0), 6)} ms  (${msPerFile(vueMean)} ms/file)`)
  console.log(`    Host (full):      ${pad(hostFullMean.toFixed(0), 6)} ms  (${msPerFile(hostFullMean)} ms/file)  ${speedupStr(vueMean, hostFullMean)}`)
  console.log(`    Host (essential): ${pad(hostEssentialMean.toFixed(0), 6)} ms  (${msPerFile(hostEssentialMean)} ms/file)  ${speedupStr(vueMean, hostEssentialMean)}`)
  console.log(`    Host (none):      ${pad(hostNoneMean.toFixed(0), 6)} ms  (${msPerFile(hostNoneMean)} ms/file)  ${speedupStr(vueMean, hostNoneMean)}`)
}

async function main() {
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  console.log('      VERTER COMPILATION BENCHMARK')
  console.log('      Vue vs VerterHost (full / essential / none)')
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')

  const fixtures = loadFixtures()
  console.log(`\nLoaded ${fixtures.length} fixtures`)

  console.log('\n── Per-fixture benchmarks ──')
  const results: FixtureResults[] = []
  for (const fixture of fixtures) {
    results.push(await benchFixture(fixture))
  }

  console.log('\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  console.log('      STRESS TEST — 20K FILES')
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  await stressTest(fixtures)

  // Summary table
  console.log('\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  console.log('      SUMMARY (mean ms per compilation)')
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  console.log('')
  const header = 'Fixture'.padEnd(22)
    + 'Vue'.padStart(10)
    + 'H:Full'.padStart(10) + 'H:Ess'.padStart(10) + 'H:None'.padStart(10)
    + '   vs Vue (host-none)'
  console.log(header)
  console.log('─'.repeat(header.length + 20))
  for (const r of results) {
    const row = r.name.padEnd(22)
      + r.vue.mean.toFixed(3).padStart(10)
      + r.hostFull.mean.toFixed(3).padStart(10)
      + r.hostEssential.mean.toFixed(3).padStart(10)
      + r.hostNone.mean.toFixed(3).padStart(10)
      + '   ' + speedupStr(r.vue.mean, r.hostNone.mean)
    console.log(row)
  }
  console.log('')

  process.exit(0)
}

main().catch(error => {
  console.error('Benchmark failed:', error)
  process.exit(1)
})
