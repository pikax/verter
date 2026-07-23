/** Bind the LSP process tree to an editor/extension-host lifetime. */
export function clientProcessLifetimeArg(pid: number): string {
  if (!Number.isSafeInteger(pid) || pid <= 0) {
    throw new Error(`invalid editor client pid: ${pid}`);
  }
  return `--client-pid=${pid}`;
}
