/**
 * Runtime constants for the MetaRuntime pooled engine system.
 * Exported for test overrides.
 */

/** How long an idle engine waits before becoming eviction-eligible (ms). */
export const IDLE_TTL_MS = 30_000;

/** How often the eviction sweep runs (ms). */
export const SWEEP_INTERVAL_MS = 5_000;

/** Soft cap on pooled engines. Exceeding logs a warning. */
export const POOL_CAP = 16;
