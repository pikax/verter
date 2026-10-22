import * as fs from "node:fs";
import {
  descendantsOf,
  isVerterLsp,
  readProcessInventory,
  type ProcessRecord,
} from "../processInventory";

/**
 * Teardown ownership: which processes the extension host's Verter language server
 * owned when the run ended, and which of them survived the editor closing.
 *
 * The extension host records its owned set as its last act (`_teardown.test.ts`);
 * the launcher — a different process, outliving the editor — reads the OS process
 * table once the editor has exited. An owned process still present then is an
 * orphan: deactivation or host exit did not reap it.
 */
export interface OwnedProcess {
  pid: number;
  name: string;
  commandLine: string;
}

export const ownedProcessFile = (logFile: string): string => `${logFile}.owned-processes.json`;

/** Every Verter LSP under `hostPid` and everything those servers spawned. */
export function ownedProcessesUnder(
  rows: readonly ProcessRecord[],
  hostPid: number,
): OwnedProcess[] {
  const owned = new Map<number, OwnedProcess>();
  for (const lsp of descendantsOf(rows, hostPid).filter(isVerterLsp)) {
    for (const row of [lsp, ...descendantsOf(rows, lsp.pid)]) {
      owned.set(row.pid, { pid: row.pid, name: row.name, commandLine: row.commandLine });
    }
  }
  return [...owned.values()];
}

/**
 * The recorded processes still in `inventory`. A match needs the same pid, name
 * and command line, so a recycled pid is not mistaken for the process it replaced.
 */
export function survivingOwned(
  recorded: readonly OwnedProcess[],
  inventory: readonly ProcessRecord[],
): OwnedProcess[] {
  const live = new Map(inventory.map((row) => [row.pid, row]));
  return recorded.filter((owned) => {
    const row = live.get(owned.pid);
    return row !== undefined && row.name === owned.name && row.commandLine === owned.commandLine;
  });
}

/**
 * Record this host's owned set for the launcher, even when it is empty: an
 * absent file then means the recording never ran, not that nothing was owned.
 */
export async function recordOwnedProcesses(logFile: string, hostPid: number): Promise<void> {
  const owned = ownedProcessesUnder(await readProcessInventory(10_000), hostPid);
  fs.writeFileSync(ownedProcessFile(logFile), JSON.stringify(owned));
}

/**
 * Fail when a process the host owned outlives the editor. The server shuts down
 * on its own after the host goes, so survivors are polled for `graceMs` first.
 * Returns the number of recorded processes that were reaped, or `undefined` when
 * no recording was written — nothing was verified, which is not the same as a
 * clean run, so the caller decides whether that run required a recording.
 */
export async function assertOwnedProcessesReaped(
  logFile: string,
  graceMs = 15_000,
): Promise<number | undefined> {
  const file = ownedProcessFile(logFile);
  if (!fs.existsSync(file)) return undefined;
  const recorded = JSON.parse(fs.readFileSync(file, "utf8")) as OwnedProcess[];
  const deadline = Date.now() + graceMs;
  let survivors = recorded;
  for (;;) {
    survivors = survivingOwned(recorded, await readProcessInventory(10_000));
    if (survivors.length === 0) return recorded.length;
    if (Date.now() >= deadline) break;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(
    `The editor exited but ${survivors.length} process(es) its Verter language server owned are ` +
      `still running: ${survivors.map((row) => `${row.name} (${row.pid})`).join(", ")}`,
  );
}
