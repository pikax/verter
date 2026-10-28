export interface DiagnosticReadiness {
  version: number;
  ready: boolean;
}

/**
 * What the wait last saw, so a deadline names where it stuck: the server never
 * answered, answered for another version, never certified the version, or
 * certified it while the editor collection kept changing.
 */
export function describeLastReadiness(observed: {
  answers: number;
  status: DiagnosticReadiness | undefined;
  editorVersion: number | undefined;
  pendingForMs: number | undefined;
  quietForMs: number;
  stableMs: number;
}): string {
  const pending =
    observed.pendingForMs === undefined
      ? ""
      : `; readiness request pending for ${observed.pendingForMs}ms`;
  const editor = observed.editorVersion === undefined ? "none" : String(observed.editorVersion);
  if (observed.answers === 0) {
    return `no readiness answer from the server${pending}; editor version=${editor}`;
  }
  if (!observed.status) {
    return `server tracks no diagnostics state for the document; editor version=${editor}${pending}`;
  }
  const { ready, version } = observed.status;
  const current = ready && version === observed.editorVersion;
  const quiet = current
    ? `, editor collection quiet for ${observed.quietForMs}ms of ${observed.stableMs}ms`
    : "";
  return `last readiness: ready=${ready}, server version=${version}, editor version=${editor}${quiet}${pending}`;
}

export async function waitForDiagnosticReceipt<T>(
  readStatus: () => PromiseLike<DiagnosticReadiness | undefined>,
  currentVersion: () => number | undefined,
  stableSince: () => number,
  readDiagnostics: () => T,
  timeoutMs: number,
  stableMs: number,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  let answers = 0;
  let lastStatus: DiagnosticReadiness | undefined;
  let requestedAt: number | undefined;
  let receiptVersion: number | undefined;
  let receiptObservedAt = 0;
  const failure = () => {
    const now = Date.now();
    const detail = describeLastReadiness({
      answers,
      status: lastStatus,
      editorVersion: currentVersion(),
      pendingForMs: requestedAt === undefined ? undefined : now - requestedAt,
      quietForMs: Math.max(0, now - Math.max(stableSince(), receiptObservedAt)),
      stableMs,
    });
    return new Error(`Diagnostics did not complete within ${timeoutMs}ms (${detail})`);
  };
  let timer: ReturnType<typeof setTimeout> | undefined;
  const poll = async () => {
    while (Date.now() < deadline) {
      requestedAt = Date.now();
      const status = await readStatus();
      requestedAt = undefined;
      answers += 1;
      lastStatus = status;
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
