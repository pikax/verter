/**
 * Workspace `.vscode/settings.json` writer plus the extension-host env handoff.
 *
 * The materialized workspace is opened by the extension host during the
 * differential run. Two things bind that host to the harness:
 *
 *  - a `.vscode/settings.json` under the workspace root that pins the same tsdk
 *    and type provider the baseline runs against, so the extension's LSP and the
 *    baseline agree on the TypeScript surface; and
 *  - the `DX_HARNESS_WORKSPACE` environment variable, set to the workspace root,
 *    which the extension host reads to discover which workspace to drive.
 */

import { mkdirSync, writeFileSync } from "node:fs";

import { canonicalizePath, joinCanonical } from "./paths.js";

/** Env var carrying the materialized workspace root to the extension host. */
export const DX_HARNESS_WORKSPACE_ENV = "DX_HARNESS_WORKSPACE";

/** Options for {@link writeWorkspaceSettings}. */
export interface WriteWorkspaceSettingsOptions {
  /** Pins `verter.typescript.tsdk` so the extension LSP uses the baseline tsdk. */
  tsdk?: string;
  /** Pins `verter.typeProvider` (e.g. `"tsgo"` / `"tsserver"`). */
  typeProvider?: string;
  /** Extra settings merged last — caller overrides win over the pinned defaults. */
  settings?: Record<string, unknown>;
}

/** The written workspace settings plus the env handoff. */
export interface WorkspaceSettings {
  /** Canonical workspace root. */
  root: string;
  /** Canonical path of the written `settings.json`. */
  settingsPath: string;
  /** The exact settings object written to disk. */
  settings: Record<string, unknown>;
  /** Extension-host env handoff: `{ DX_HARNESS_WORKSPACE: <root> }`. */
  env: Record<string, string>;
}

/**
 * Write `<root>/.vscode/settings.json` and return it together with the
 * `DX_HARNESS_WORKSPACE` env handoff. `verter.server.logLevel` defaults to
 * `"debug"`; unset tsdk/provider pins are omitted; caller `settings` overrides
 * are merged last (so a caller may override the pinned log level).
 */
export function writeWorkspaceSettings(
  root: string,
  opts: WriteWorkspaceSettingsOptions = {},
): WorkspaceSettings {
  const canonicalRoot = canonicalizePath(root);

  // Pin debug logging by default: the extension run transport copies
  // `verter.server.logLevel` (package default "info", see
  // packages/vue-vscode/src/extension.ts) into the server's VERTER_LOG, so the
  // extension-host readiness/log gates need "debug" to keep their signal. Seeded
  // first so an explicit caller `settings` override still wins.
  const settings: Record<string, unknown> = { "verter.server.logLevel": "debug" };
  if (opts.tsdk !== undefined) settings["verter.typescript.tsdk"] = canonicalizePath(opts.tsdk);
  if (opts.typeProvider !== undefined) settings["verter.typeProvider"] = opts.typeProvider;
  Object.assign(settings, opts.settings ?? {});

  const vscodeDir = joinCanonical(canonicalRoot, ".vscode");
  const settingsPath = joinCanonical(vscodeDir, "settings.json");
  mkdirSync(vscodeDir, { recursive: true });
  writeFileSync(settingsPath, JSON.stringify(settings, null, 2) + "\n", "utf-8");

  return {
    root: canonicalRoot,
    settingsPath,
    settings,
    env: { [DX_HARNESS_WORKSPACE_ENV]: canonicalRoot },
  };
}
