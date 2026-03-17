import { createHash } from "crypto";
import { createRequire } from "module";
import type {
  VerterHost,
  Workspace,
  ProcessStyleOptions,
  ProcessStyleResult,
} from "@verter/native";

const require = createRequire(import.meta.url);

type NativeModule = typeof import("@verter/native");

let native: NativeModule | null = null;
let host: VerterHost | null = null;
let workspace: Workspace | null = null;

function loadNative(): NativeModule {
  if (native) return native;
  native = require("@verter/native") as NativeModule;
  return native;
}

export function loadWorkspace(roots: string[]): Workspace {
  if (workspace) return workspace;
  const n = loadNative();
  workspace = new n.Workspace(roots);
  return workspace;
}

export function getWorkspace(): Workspace | null {
  return workspace;
}

export function loadHost(config?: { devMode?: boolean }): VerterHost {
  if (host) return host;
  const n = loadNative();
  const hostConfig = { devMode: config?.devMode ?? true };
  host = workspace
    ? n.VerterHost.withWorkspace(hostConfig, workspace)
    : new n.VerterHost(hostConfig);
  return host;
}

export function resetHost(): void {
  host?.close();
  host = null;
}

export function processStyle(css: string, options: ProcessStyleOptions): ProcessStyleResult {
  return loadNative().processStyle(css, options);
}

export function getHash(text: string): string {
  return createHash("sha256").update(text).digest("hex").substring(0, 8);
}

export function generateComponentId(
  filename: string,
  source: string,
  isProd: boolean,
  root?: string,
): string {
  const normalized = filename.replace(/\\/g, "/");
  const normalizedRoot = root?.replace(/\\/g, "/").replace(/\/$/, "");
  // Compute relative path (matching @vitejs/plugin-vue):
  // path.relative(root, filename) with forward slashes
  const relativePath =
    normalizedRoot && normalized.startsWith(normalizedRoot + "/")
      ? normalized.slice(normalizedRoot.length + 1)
      : normalized;
  // Vue: dev = hash(path), prod = hash(path + source)
  return isProd ? getHash(relativePath + source) : getHash(relativePath);
}
