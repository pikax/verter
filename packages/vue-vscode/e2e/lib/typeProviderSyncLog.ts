export type TypeProviderSyncProgress = "awaiting-ready" | "awaiting-sync" | "complete";

/**
 * Whether the server that is running NOW has announced a completed type-provider
 * sync, judged from the E2E log.
 *
 * `since` is the log offset taken BEFORE the most recent language-server restart
 * (0 when there was none). Init generations restart at 1 with every server
 * process, so lines written by an earlier process carry the very numbers the
 * restarted one will use and must never be consulted.
 */
export function typeProviderSyncCompleteSince(
  log: string,
  since: number,
): TypeProviderSyncProgress {
  const current = log.slice(since);
  let readyGeneration: number | undefined;
  for (const match of current.matchAll(/Verter ready \(init generation (\d+)\)/g)) {
    const generation = parseInt(match[1], 10);
    if (readyGeneration === undefined || generation > readyGeneration) {
      readyGeneration = generation;
    }
  }
  if (readyGeneration === undefined) return "awaiting-ready";
  for (const match of current.matchAll(/TypeProviderSyncComplete \(init generation (\d+)\)/g)) {
    if (parseInt(match[1], 10) >= readyGeneration) return "complete";
  }
  return "awaiting-sync";
}
