import type { BenchmarkStats } from './stats'
import { formatDuration, formatBytes, formatMemory, calculateSpeedup, calculateThroughput } from './stats'

export interface FixtureResult {
  name: string
  size: number
  vue: BenchmarkResult
  verter: BenchmarkResult
  speedup: number
  status: 'pass' | 'warning' | 'fail'
}

export interface BenchmarkResult {
  stats: BenchmarkStats
  opsPerSec: number
  throughputMBs: number
  errors: string[]
}

export interface BenchmarkReport {
  fixtures: FixtureResult[]
  summary: {
    totalFixtures: number
    passed: number
    warnings: number
    failed: number
    avgSpeedup: number
    overallStatus: 'pass' | 'warning' | 'fail'
  }
  timestamp: string
}

/**
 * Determine status based on speedup factor
 * - Pass: Verter >= 100% of Vue performance (speedup >= 1.0)
 * - Warning: Verter 50-99% of Vue performance (0.5 <= speedup < 1.0)
 * - Fail: Verter < 50% of Vue performance (speedup < 0.5)
 */
export function determineStatus(speedup: number): 'pass' | 'warning' | 'fail' {
  if (speedup >= 1.0) return 'pass'
  if (speedup >= 0.5) return 'warning'
  return 'fail'
}

/**
 * Generate markdown report
 */
export function generateMarkdownReport(report: BenchmarkReport): string {
  const lines: string[] = []

  lines.push('# Verter Benchmark Results')
  lines.push('')
  lines.push(`**Generated:** ${report.timestamp}`)
  lines.push('')

  // Summary
  const { summary } = report
  const statusEmoji = {
    pass: '✅',
    warning: '⚠️',
    fail: '❌'
  }

  lines.push('## Summary')
  lines.push('')
  lines.push(`**Overall Status:** ${statusEmoji[summary.overallStatus]} ${summary.overallStatus.toUpperCase()}`)
  lines.push('')
  lines.push(`- Total Fixtures: ${summary.totalFixtures}`)
  lines.push(`- ${statusEmoji.pass} Passed: ${summary.passed}`)
  lines.push(`- ${statusEmoji.warning} Warnings: ${summary.warnings}`)
  lines.push(`- ${statusEmoji.fail} Failed: ${summary.failed}`)
  lines.push(`- Average Speedup: ${summary.avgSpeedup.toFixed(2)}x`)
  lines.push('')

  // Detailed Results
  lines.push('## Detailed Results')
  lines.push('')
  lines.push('| Fixture | Size | Vue (ms) | Verter (ms) | Memory | Speedup | Throughput | Status |')
  lines.push('|---------|------|----------|-------------|--------|---------|------------|--------|')

  for (const fixture of report.fixtures) {
    const vueMean = fixture.vue.stats.mean.toFixed(2)
    const verterMean = fixture.verter.stats.mean.toFixed(2)
    const speedup = fixture.speedup.toFixed(2)
    const throughput = fixture.verter.throughputMBs.toFixed(2)
    const memory = formatMemory(fixture.verter.stats.heapUsedMB * 1024 * 1024)
    const status = statusEmoji[fixture.status]

    lines.push(
      `| ${fixture.name} | ${formatBytes(fixture.size)} | ${vueMean} | ${verterMean} | ${memory} | ${speedup}x | ${throughput} MB/s | ${status} |`
    )
  }
  lines.push('')

  // Performance Details
  lines.push('## Performance Details')
  lines.push('')

  for (const fixture of report.fixtures) {
    lines.push(`### ${fixture.name}`)
    lines.push('')
    lines.push('**Vue:**')
    lines.push(`- Mean: ${formatDuration(fixture.vue.stats.mean)}`)
    lines.push(`- Median (p50): ${formatDuration(fixture.vue.stats.median)}`)
    lines.push(`- p95: ${formatDuration(fixture.vue.stats.p95)}`)
    lines.push(`- p99: ${formatDuration(fixture.vue.stats.p99)}`)
    lines.push(`- Ops/sec: ${fixture.vue.opsPerSec.toFixed(0)}`)
    lines.push(`- Throughput: ${fixture.vue.throughputMBs.toFixed(2)} MB/s`)
    lines.push(`- Memory: ${formatMemory(fixture.vue.stats.heapUsedMB * 1024 * 1024)}`)
    lines.push('')
    lines.push('**Verter:**')
    lines.push(`- Mean: ${formatDuration(fixture.verter.stats.mean)}`)
    lines.push(`- Median (p50): ${formatDuration(fixture.verter.stats.median)}`)
    lines.push(`- p95: ${formatDuration(fixture.verter.stats.p95)}`)
    lines.push(`- p99: ${formatDuration(fixture.verter.stats.p99)}`)
    lines.push(`- Ops/sec: ${fixture.verter.opsPerSec.toFixed(0)}`)
    lines.push(`- Throughput: ${fixture.verter.throughputMBs.toFixed(2)} MB/s`)
    lines.push(`- Memory: ${formatMemory(fixture.verter.stats.heapUsedMB * 1024 * 1024)}`)
    lines.push('')
    lines.push(`**Speedup:** ${fixture.speedup.toFixed(2)}x (${statusEmoji[fixture.status]} ${fixture.status})`)
    lines.push('')
  }

  // Status Criteria
  lines.push('## Status Criteria')
  lines.push('')
  lines.push('- ✅ **Pass**: Verter ≥ 100% of Vue performance (speedup ≥ 1.0x)')
  lines.push('- ⚠️ **Warning**: Verter 50-99% of Vue performance (0.5x ≤ speedup < 1.0x)')
  lines.push('- ❌ **Fail**: Verter < 50% of Vue performance (speedup < 0.5x)')
  lines.push('')

  return lines.join('\n')
}

/**
 * Generate JSON report
 */
export function generateJsonReport(report: BenchmarkReport): string {
  return JSON.stringify(report, null, 2)
}

/**
 * Generate console output
 */
export function generateConsoleOutput(report: BenchmarkReport): string {
  const lines: string[] = []

  const statusColors = {
    pass: '\x1b[32m', // green
    warning: '\x1b[33m', // yellow
    fail: '\x1b[31m', // red
    reset: '\x1b[0m'
  }

  lines.push('')
  lines.push('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  lines.push('           VERTER BENCHMARK RESULTS')
  lines.push('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  lines.push('')

  const { summary } = report
  const statusColor = statusColors[summary.overallStatus]
  
  lines.push(`Overall Status: ${statusColor}${summary.overallStatus.toUpperCase()}${statusColors.reset}`)
  lines.push(`Average Speedup: ${summary.avgSpeedup.toFixed(2)}x`)
  lines.push(`Passed: ${summary.passed} | Warnings: ${summary.warnings} | Failed: ${summary.failed}`)
  lines.push('')

  for (const fixture of report.fixtures) {
    const statusColor = statusColors[fixture.status]
    const statusSymbol = fixture.status === 'pass' ? '✓' : fixture.status === 'warning' ? '⚠' : '✗'
    
    lines.push(`${statusColor}${statusSymbol}${statusColors.reset} ${fixture.name}`)
    lines.push(`  Size: ${formatBytes(fixture.size)}`)
    lines.push(`  Vue:    ${fixture.vue.stats.mean.toFixed(2)} ms (${fixture.vue.opsPerSec.toFixed(0)} ops/s, ${fixture.vue.throughputMBs.toFixed(2)} MB/s) - ${formatMemory(fixture.vue.stats.heapUsedMB * 1024 * 1024)}`)
    lines.push(`  Verter: ${fixture.verter.stats.mean.toFixed(2)} ms (${fixture.verter.opsPerSec.toFixed(0)} ops/s, ${fixture.verter.throughputMBs.toFixed(2)} MB/s) - ${formatMemory(fixture.verter.stats.heapUsedMB * 1024 * 1024)}`)
    lines.push(`  Speedup: ${statusColor}${fixture.speedup.toFixed(2)}x${statusColors.reset}`)
    lines.push('')
  }

  lines.push('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━')
  lines.push('')

  return lines.join('\n')
}
