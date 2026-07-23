export interface TypeScriptPluginRefreshScheduler {
  request(): void;
  flush(): void;
  dispose(): void;
}

export interface TypeScriptPluginRefreshSchedulerOptions {
  readonly idleDelayMs: number;
  readonly maximumDelayMs: number;
}

const DEFAULT_OPTIONS: TypeScriptPluginRefreshSchedulerOptions = {
  idleDelayMs: 50,
  maximumDelayMs: 250,
};

/**
 * Capacity-one trailing scheduler for carrier-store invalidation.
 *
 * Workspace scans can publish many immutable carrier snapshots in quick
 * succession. VS Code's tsserver needs one plugin configuration fence for the
 * latest store state, not one protocol command per file. The trailing edge
 * absorbs bursts while the maximum edge guarantees continuous publication can
 * never starve import discovery.
 */
export function createTypeScriptPluginRefreshScheduler(
  refresh: () => void,
  options: TypeScriptPluginRefreshSchedulerOptions = DEFAULT_OPTIONS,
): TypeScriptPluginRefreshScheduler {
  let trailingTimer: ReturnType<typeof setTimeout> | undefined;
  let maximumTimer: ReturnType<typeof setTimeout> | undefined;
  let pending = false;
  let disposed = false;

  const clearTimers = () => {
    if (trailingTimer !== undefined) clearTimeout(trailingTimer);
    if (maximumTimer !== undefined) clearTimeout(maximumTimer);
    trailingTimer = undefined;
    maximumTimer = undefined;
  };

  const flush = () => {
    if (disposed || !pending) return;
    pending = false;
    clearTimers();
    refresh();
  };

  return {
    request() {
      if (disposed) return;
      pending = true;
      if (trailingTimer !== undefined) clearTimeout(trailingTimer);
      trailingTimer = setTimeout(flush, options.idleDelayMs);
      maximumTimer ??= setTimeout(flush, options.maximumDelayMs);
    },
    flush,
    dispose() {
      if (disposed) return;
      disposed = true;
      pending = false;
      clearTimers();
    },
  };
}
