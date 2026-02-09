import { compressToEncodedURIComponent, decompressFromEncodedURIComponent } from "lz-string";
import type { CompilerOptions, OutputMode } from "./types";
import type { ImportMap } from "./importMap";

export interface SerializedState {
  files: Record<string, string>; // filename -> code
  activeFile: string;
  outputMode: OutputMode;
  compilerOptions: CompilerOptions;
  importMap?: ImportMap;
}

export function serializeToHash(state: SerializedState): void {
  const json = JSON.stringify(state);
  const compressed = compressToEncodedURIComponent(json);
  history.replaceState(null, "", `#${compressed}`);
}

export function deserializeFromHash(): SerializedState | null {
  const hash = location.hash.slice(1);
  if (!hash) return null;

  try {
    const json = decompressFromEncodedURIComponent(hash);
    if (!json) return null;
    return JSON.parse(json) as SerializedState;
  } catch {
    return null;
  }
}
