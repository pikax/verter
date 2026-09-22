export async function waitForDiagnosticReceipt<T>(
  readStatus: () => PromiseLike<{ version: number; ready: boolean } | undefined>,
  currentVersion: () => number | undefined,
  stableSince: () => number,
  readDiagnostics: () => T,
  timeoutMs: number,
  stableMs: number,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  const failure = () => new Error(`Diagnostics did not complete within ${timeoutMs}ms`);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let receiptVersion: number | undefined;
  let receiptObservedAt = 0;
  const poll = async () => {
    while (Date.now() < deadline) {
      const status = await readStatus();
      if (status?.ready && status.version === currentVersion()) {
        if (receiptVersion !== status.version) receiptObservedAt = Date.now();
        receiptVersion = status.version;
      } else {
        receiptVersion = undefined;
      }
      if (
        Date.now() < deadline &&
        status?.ready &&
        status.version === currentVersion() &&
        Date.now() - Math.max(stableSince(), receiptObservedAt) >= stableMs
      ) {
        return readDiagnostics();
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    throw failure();
  };
  try {
    return await Promise.race([
      poll(),
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(failure()), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
