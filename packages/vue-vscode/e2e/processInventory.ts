import { execFile } from "node:child_process";
import { basename } from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

export interface ProcessRecord {
  pid: number;
  parentPid: number;
  name: string;
  commandLine: string;
}

interface WindowsProcessRow {
  ProcessId?: unknown;
  ParentProcessId?: unknown;
  Name?: unknown;
  CommandLine?: unknown;
}

export function parseWindowsProcessInventory(raw: string): ProcessRecord[] {
  const parsed = JSON.parse(raw) as WindowsProcessRow | WindowsProcessRow[] | null;
  const rows = parsed == null ? [] : Array.isArray(parsed) ? parsed : [parsed];
  return rows.flatMap((row) => {
    const pid = Number(row.ProcessId);
    const parentPid = Number(row.ParentProcessId);
    if (!Number.isSafeInteger(pid) || pid <= 0 || !Number.isSafeInteger(parentPid)) {
      return [];
    }
    return [
      {
        pid,
        parentPid,
        name: typeof row.Name === "string" ? row.Name : "",
        commandLine: typeof row.CommandLine === "string" ? row.CommandLine : "",
      },
    ];
  });
}

export function parsePosixProcessInventory(raw: string): ProcessRecord[] {
  const rows: ProcessRecord[] = [];
  for (const line of raw.split(/\r?\n/)) {
    const match = /^\s*(\d+)\s+(\d+)\s+(.+?)\s*$/.exec(line);
    if (!match) continue;
    const commandLine = match[3];
    const executable = /^(?:"([^"]+)"|'([^']+)'|(\S+))/.exec(commandLine);
    const executablePath = executable?.[1] ?? executable?.[2] ?? executable?.[3] ?? "";
    rows.push({
      pid: Number(match[1]),
      parentPid: Number(match[2]),
      name: basename(executablePath),
      commandLine,
    });
  }
  return rows;
}

export function descendantsOf(
  rows: readonly ProcessRecord[],
  ancestorPid: number,
): ProcessRecord[] {
  const children = new Map<number, ProcessRecord[]>();
  for (const row of rows) {
    const bucket = children.get(row.parentPid);
    if (bucket) bucket.push(row);
    else children.set(row.parentPid, [row]);
  }

  const descendants: ProcessRecord[] = [];
  const pending = [...(children.get(ancestorPid) ?? [])];
  const visited = new Set<number>();
  while (pending.length > 0) {
    const current = pending.shift()!;
    if (visited.has(current.pid)) continue;
    visited.add(current.pid);
    descendants.push(current);
    pending.push(...(children.get(current.pid) ?? []));
  }
  return descendants;
}

export function localSemanticEnginesUnderVerterLsp(
  rows: readonly ProcessRecord[],
): ProcessRecord[] {
  const engines = new Map<number, ProcessRecord>();
  for (const lsp of rows.filter(isVerterLsp)) {
    for (const descendant of descendantsOf(rows, lsp.pid)) {
      if (isSemanticEngine(descendant)) engines.set(descendant.pid, descendant);
    }
  }
  return [...engines.values()];
}

export async function readProcessInventory(timeoutMs = 5_000): Promise<ProcessRecord[]> {
  if (process.platform === "win32") {
    const script =
      "Get-CimInstance Win32_Process | " +
      "Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress";
    const { stdout } = await execFileAsync(
      "powershell.exe",
      ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script],
      { timeout: timeoutMs, windowsHide: true, maxBuffer: 16 * 1024 * 1024 },
    );
    return parseWindowsProcessInventory(stdout);
  }

  const { stdout } = await execFileAsync("ps", ["-eo", "pid=,ppid=,command="], {
    timeout: timeoutMs,
    maxBuffer: 16 * 1024 * 1024,
  });
  return parsePosixProcessInventory(stdout);
}

function isVerterLsp(row: ProcessRecord): boolean {
  return /^verter-lsp(?:\.exe)?$/i.test(row.name);
}

function isSemanticEngine(row: ProcessRecord): boolean {
  if (/^(?:node|code|electron)(?:\.exe)?$/i.test(row.name)) {
    return /(?:^|[\\/])tsserver(?:library)?\.js(?:["']|\s|$)/i.test(row.commandLine);
  }
  return (
    /^(?:tsgo|tsc)(?:\.exe)?$/i.test(row.name) && /(?:^|\s)--lsp(?:\s|$)/i.test(row.commandLine)
  );
}
