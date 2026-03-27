export interface BenchmarkStats {
  mean: number;
  median: number;
  p95: number;
  p99: number;
  min: number;
  max: number;
  stdDev: number;
  heapUsedMB: number; // Change in heap memory used (MB)
}

/**
 * Calculate throughput in MB/s
 */
export function calculateThroughput(sizeBytes: number, timeMs: number): number {
  if (timeMs === 0) return 0;
  const timeSec = timeMs / 1000;
  const sizeMB = sizeBytes / (1024 * 1024);
  return sizeMB / timeSec;
}

/**
 * Calculate speedup factor (how many times faster)
 */
export function calculateSpeedup(baselineMs: number, comparisonMs: number): number {
  if (comparisonMs === 0) return 0;
  return baselineMs / comparisonMs;
}

/**
 * Format bytes to human readable string
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(2)} ${sizes[i]}`;
}

/**
 * Format memory size to human readable string
 */
export function formatMemory(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

/**
 * Format duration to human readable string
 */
export function formatDuration(ms: number): string {
  if (ms < 1) return `${(ms * 1000).toFixed(2)} µs`;
  if (ms < 1000) return `${ms.toFixed(2)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}
