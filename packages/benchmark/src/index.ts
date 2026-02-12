import { Bench } from 'tinybench'
import { readFileSync, writeFileSync } from 'fs'
import { join, dirname } from 'path'
import { fileURLToPath } from 'url'
import { compileVue } from './compilers/vue'
import { compileVerter } from './compilers/verter'
import { calculateThroughput, calculateSpeedup, formatMemory } from './utils/stats'
import { 
  generateMarkdownReport, 
  generateJsonReport, 
  generateConsoleOutput,
  determineStatus,
  type BenchmarkReport,
  type FixtureResult,
  type BenchmarkResult
} from './utils/report'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

// Fixture files
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

interface FixtureBenchmark {
  name: string
  source: string
  size: number
  iterations?: number
}

/**
 * Load all fixture files
 */
function loadFixtures(): FixtureBenchmark[] {
  const fixturesDir = join(__dirname, 'fixtures')
  
  return FIXTURES.map(filename => {
    const filepath = join(fixturesDir, filename)
    const source = readFileSync(filepath, 'utf-8')
    const size = Buffer.byteLength(source, 'utf-8')
    
    return {
      name: filename.replace('.vue', ''),
      source,
      size,
      iterations: 200 // Default: 200 samples for individual fixtures
    }
  })
}

/**
 * Create a stress test fixture with ~20k files
 * Repeats all fixtures to reach approximately 20k files
 */
function createStressFixture(fixtures: FixtureBenchmark[]): FixtureBenchmark {
  const targetFiles = 20000
  const timesPerFixture = Math.ceil(targetFiles / fixtures.length)
  
  // Create an array of all sources repeated
  const allSources: string[] = []
  for (let i = 0; i < timesPerFixture; i++) {
    for (const fixture of fixtures) {
      allSources.push(fixture.source)
    }
  }
  
  const totalSize = allSources.reduce((sum, src) => sum + Buffer.byteLength(src, 'utf-8'), 0)
  
  return {
    name: `stress-test-${allSources.length}-files`,
    source: JSON.stringify(allSources), // Store as JSON array string
    size: totalSize,
    iterations: 1 // Fewer samples for stress test
  }
}

/**
 * Run benchmarks for a single fixture
 */
async function benchmarkFixture(fixture: FixtureBenchmark, isStress: boolean = false): Promise<FixtureResult> {
  const sizeDisplay = fixture.size > 1024 * 1024 
    ? `${(fixture.size / (1024 * 1024)).toFixed(2)} MB`
    : `${(fixture.size / 1024).toFixed(2)} KB`
  
  console.log(`\nBenchmarking ${fixture.name}... (${sizeDisplay})`)
  
  const iterations = fixture.iterations || 50
  const warmupIterations = isStress ? 0 : 10

  const vueErrors: string[] = []
  const verterErrors: string[] = []

  // Benchmark Vue
  if (global.gc) global.gc()
  const vueMemBefore = process.memoryUsage()
  
  const vueBench = new Bench({
    time: isStress ? 50000 : 1000,
    warmupIterations,
    iterations
  })

  if (isStress) {
    const sources: string[] = JSON.parse(fixture.source)
    console.log(`  Compiling ${sources.length} files per iteration...`)
    vueBench.add(`Vue-${fixture.name}`, () => {
      for (const source of sources) {
        const result = compileVue(source, 'stress-test.vue')
        if (result.errors.length > 0 && vueErrors.length === 0) {
          vueErrors.push(...result.errors.slice(0, 3))
        }
      }
    })
  } else {
    vueBench.add(`Vue-${fixture.name}`, () => {
      const result = compileVue(fixture.source, `${fixture.name}.vue`)
      if (result.errors.length > 0 && vueErrors.length === 0) {
        vueErrors.push(...result.errors)
      }
    })
  }

  await vueBench.run()
  const vueMemAfter = process.memoryUsage()
  const vueHeapUsedMB = (vueMemAfter.heapUsed - vueMemBefore.heapUsed) / (1024 * 1024)

  // Benchmark Verter
  if (global.gc) global.gc()
  const verterMemBefore = process.memoryUsage()
  
  const verterBench = new Bench({
    time: isStress ? 50000 : 1000,
    warmupIterations,
    iterations
  })

  if (isStress) {
    const sources: string[] = JSON.parse(fixture.source)
    verterBench.add(`Verter-${fixture.name}`, () => {
      for (const source of sources) {
        const result = compileVerter(source, 'stress-test.vue')
        if (result.errors.length > 0 && verterErrors.length === 0) {
          verterErrors.push(...result.errors.slice(0, 3))
        }
      }
    })
  } else {
    verterBench.add(`Verter-${fixture.name}`, () => {
      const result = compileVerter(fixture.source, `${fixture.name}.vue`)
      if (result.errors.length > 0 && verterErrors.length === 0) {
        verterErrors.push(...result.errors)
      }
    })
  }

  await verterBench.run()
  const verterMemAfter = process.memoryUsage()
  const verterHeapUsedMB = (verterMemAfter.heapUsed - verterMemBefore.heapUsed) / (1024 * 1024)

  // Extract statistics from tinybench results
  const vueBenchResult = vueBench.tasks.find(t => t.name === `Vue-${fixture.name}`)!
  const verterBenchResult = verterBench.tasks.find(t => t.name === `Verter-${fixture.name}`)!

  const vueResult: BenchmarkResult = {
    stats: {
      mean: vueBenchResult.result?.mean || 0,
      median: vueBenchResult.result?.mean || 0, // tinybench doesn't provide median directly
      p95: vueBenchResult.result?.mean || 0,
      p99: vueBenchResult.result?.mean || 0,
      min: vueBenchResult.result?.min || 0,
      max: vueBenchResult.result?.max || 0,
      stdDev: vueBenchResult.result?.variance ? Math.sqrt(vueBenchResult.result.variance) : 0,
      heapUsedMB: Math.max(0, vueHeapUsedMB) // Heap delta can sometimes be negative
    },
    opsPerSec: vueBenchResult.result?.hz || 0,
    throughputMBs: calculateThroughput(fixture.size, vueBenchResult.result?.mean || 0),
    errors: vueErrors
  }

  const verterResult: BenchmarkResult = {
    stats: {
      mean: verterBenchResult.result?.mean || 0,
      median: verterBenchResult.result?.mean || 0,
      p95: verterBenchResult.result?.mean || 0,
      p99: verterBenchResult.result?.mean || 0,
      min: verterBenchResult.result?.min || 0,
      max: verterBenchResult.result?.max || 0,
      stdDev: verterBenchResult.result?.variance ? Math.sqrt(verterBenchResult.result.variance) : 0,
      heapUsedMB: Math.max(0, verterHeapUsedMB)
    },
    opsPerSec: verterBenchResult.result?.hz || 0,
    throughputMBs: calculateThroughput(fixture.size, verterBenchResult.result?.mean || 0),
    errors: verterErrors
  }

  // Calculate speedup (Verter relative to Vue)
  const speedup = calculateSpeedup(vueResult.stats.mean, verterResult.stats.mean)
  const status = determineStatus(speedup)

  if (isStress) {
    const sources: string[] = JSON.parse(fixture.source)
    const filesPerIteration = sources.length
    console.log(`  Vue:    ${vueResult.stats.mean.toFixed(2)} ms for ${filesPerIteration} files (${(vueResult.stats.mean / filesPerIteration).toFixed(3)} ms/file, ${vueResult.throughputMBs.toFixed(2)} MB/s) - ${formatMemory(vueResult.stats.heapUsedMB * 1024 * 1024)}`)
    console.log(`  Verter: ${verterResult.stats.mean.toFixed(2)} ms for ${filesPerIteration} files (${(verterResult.stats.mean / filesPerIteration).toFixed(3)} ms/file, ${verterResult.throughputMBs.toFixed(2)} MB/s) - ${formatMemory(verterResult.stats.heapUsedMB * 1024 * 1024)}`)
    console.log(`  Speedup: ${speedup.toFixed(2)}x - ${status}`)
  } else {
    console.log(`  Vue:    ${vueResult.stats.mean.toFixed(2)} ms (${vueResult.opsPerSec.toFixed(0)} ops/s, ${vueResult.throughputMBs.toFixed(2)} MB/s) - ${formatMemory(vueResult.stats.heapUsedMB * 1024 * 1024)}`)
    console.log(`  Verter: ${verterResult.stats.mean.toFixed(2)} ms (${verterResult.opsPerSec.toFixed(0)} ops/s, ${verterResult.throughputMBs.toFixed(2)} MB/s) - ${formatMemory(verterResult.stats.heapUsedMB * 1024 * 1024)}`)
    console.log(`  Speedup: ${speedup.toFixed(2)}x - ${status}`)
  }

  return {
    name: fixture.name,
    size: fixture.size,
    vue: vueResult,
    verter: verterResult,
    speedup,
    status
  }
}

/**
 * Main benchmark runner
 */
async function main() {
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  console.log('           VERTER COMPILATION BENCHMARK')
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  console.log('')
  console.log('Comparing Vue (@vue/compiler-sfc) vs Verter (Rust/NAPI)')
  console.log('')

  const fixtures = loadFixtures()
  console.log(`Loaded ${fixtures.length} fixtures`)

  const results: FixtureResult[] = []

  for (const fixture of fixtures) {
    const result = await benchmarkFixture(fixture, false)
    results.push(result)
  }

  // Always run stress test
  console.log('\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  console.log('              STRESS TEST - 20K FILES')
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  
  const stressFixture = createStressFixture(fixtures)
  const stressResult = await benchmarkFixture(stressFixture, true)
  results.push(stressResult)

  // Calculate summary
  const passed = results.filter(r => r.status === 'pass').length
  const warnings = results.filter(r => r.status === 'warning').length
  const failed = results.filter(r => r.status === 'fail').length
  const avgSpeedup = results.reduce((sum, r) => sum + r.speedup, 0) / results.length

  let overallStatus: 'pass' | 'warning' | 'fail'
  if (failed > 0) {
    overallStatus = 'fail'
  } else if (warnings > 0) {
    overallStatus = 'warning'
  } else {
    overallStatus = 'pass'
  }

  const report: BenchmarkReport = {
    fixtures: results,
    summary: {
      totalFixtures: results.length,
      passed,
      warnings,
      failed,
      avgSpeedup,
      overallStatus
    },
    timestamp: new Date().toISOString()
  }

  // Output console summary
  console.log(generateConsoleOutput(report))

  // Check if --json flag is provided
  const jsonOutput = process.argv.includes('--json')

  if (jsonOutput) {
    // Write JSON report to stdout for CI
    console.log(generateJsonReport(report))
  } else {
    // Write markdown report to file
    const outputDir = join(process.cwd(), 'benchmark-results')
    const markdownPath = join(outputDir, 'results.md')
    const jsonPath = join(outputDir, 'results.json')

    try {
      // Create output directory if it doesn't exist
      const { mkdirSync, existsSync } = await import('fs')
      if (!existsSync(outputDir)) {
        mkdirSync(outputDir, { recursive: true })
      }

      writeFileSync(markdownPath, generateMarkdownReport(report))
      writeFileSync(jsonPath, generateJsonReport(report))

      console.log(`\n📊 Reports saved:`)
      console.log(`   - ${markdownPath}`)
      console.log(`   - ${jsonPath}`)
    } catch (error) {
      console.error('Failed to write reports:', error)
    }
  }

  // Exit with code 0 (don't fail CI on performance issues)
  // Performance issues should be reviewed but not block builds
  process.exit(0)
}

main().catch(error => {
  console.error('Benchmark failed:', error)
  process.exit(1)
})
