export async function pollUntilWithin<T>(
  label: string,
  request: () => Promise<T>,
  ready: (value: T) => boolean,
  timeoutMs: number,
  intervalMs = 150,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;

  while (true) {
    const latest = await request();
    // An in-flight provider request cannot be preempted by this helper. If it
    // completes with the requested value just after the polling deadline, that
    // is a slow success rather than a timeout. Check the completed observation
    // before deciding whether another poll may begin.
    if (ready(latest)) return latest;
    const remainingMs = deadline - Date.now();
    if (remainingMs <= 0) {
      throw new Error(`${label} not ready within ${timeoutMs}ms`);
    }
    await new Promise((resolve) => setTimeout(resolve, Math.min(intervalMs, remainingMs)));
  }
}

export function semanticHoverReady(
  text: string,
  needles: readonly string[],
  options?: { forbidAny?: boolean; forbidUnknown?: boolean; forbidGenerated?: boolean },
): boolean {
  return (
    text.length > 0 &&
    needles.every((needle) => text.includes(needle)) &&
    (options?.forbidAny === false || !/:\s*any\b/.test(text)) &&
    (!options?.forbidUnknown || !/\bunknown\b/.test(text)) &&
    (options?.forbidGenerated === false || !/__Verter\w*/.test(text))
  );
}
