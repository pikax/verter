export interface BenchmarkStats {
  mean: number
  median: number
  p95: number
  p99: number
  min: number
  max: number
  stdDev: number
  heapUsedMB: number // Change in heap memory used (MB)
}

/**
 * Calculate statistical metrics from an array of measurements
 */
export function calculateStats(samples: number[], heapUsedMB: number = 0): BenchmarkStats {
  if (samples.length === 0) {
    return {
      mean: 0,
      median: 0,
      p95: 0,
      p99: 0,
      min: 0,
      max: 0,
      stdDev: 0,
      heapUsedMB
    }
  }

  const sorted = [...samples].sort((a, b) => a - b)
  const len = sorted.length

  const mean = samples.reduce((sum, val) => sum + val, 0) / len
  const median = getPercentile(sorted, 50)
  const p95 = getPercentile(sorted, 95)
  const p99 = getPercentile(sorted, 99)
  const min = sorted[0]
  const max = sorted[len - 1]

  // Calculate standard deviation
  const squaredDiffs = samples.map(val => Math.pow(val - mean, 2))
  const variance = squaredDiffs.reduce((sum, val) => sum + val, 0) / len
  const stdDev = Math.sqrt(variance)

  return {
    mean,
    median,
    p95,
    p99,
    min,
    max,
    stdDev,
    heapUsedMB
  }
}

/**
 * Get percentile value from sorted array
 */
function getPercentile(sorted: number[], percentile: number): number {
  const index = (percentile / 100) * (sorted.length - 1)
  const lower = Math.floor(index)
  const upper = Math.ceil(index)
  const weight = index - lower

  if (lower === upper) {
    return sorted[lower]
  }

  return sorted[lower] * (1 - weight) + sorted[upper] * weight
}

/**
 * Calculate throughput in MB/s
 */
export function calculateThroughput(sizeBytes: number, timeMs: number): number {
  if (timeMs === 0) return 0
  const timeSec = timeMs / 1000
  const sizeMB = sizeBytes / (1024 * 1024)
  return sizeMB / timeSec
}

/**
 * Calculate speedup factor (how many times faster)
 */
export function calculateSpeedup(baselineMs: number, comparisonMs: number): number {
  if (comparisonMs === 0) return 0
  return baselineMs / comparisonMs
}

/**
 * Format bytes to human readable string
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return `${(bytes / Math.pow(k, i)).toFixed(2)} ${sizes[i]}`
}

/**
 * Format memory size to human readable string
 */
export function formatMemory(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}

/**
 * Format duration to human readable string
 */
export function formatDuration(ms: number): string {
  if (ms < 1) return `${(ms * 1000).toFixed(2)} µs`
  if (ms < 1000) return `${ms.toFixed(2)} ms`
  return `${(ms / 1000).toFixed(2)} s`
}
