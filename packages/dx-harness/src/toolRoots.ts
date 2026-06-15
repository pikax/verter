/**
 * Deterministic TypeScript tool-root resolution for the differential baseline.
 *
 * The `verter_dx_baseline` bridge pins the TS provider against an explicit tool
 * root and refuses any discovered path that does not match it (no silent drift
 * onto an ambient/global TypeScript). B computes that tool root here and hands it
 * to the bridge in `hello`; C validates it. The dependency source mirrors the
 * shipped extension, which always passes `--tsdk` as either the user setting or
 * the bundled extension TypeScript lib (`<extensionPath>/node_modules/typescript/lib`,
 * see `packages/vue-vscode/src/extension.ts`). The harness resolves that SAME
 * bundled lib — `<repoRoot>/packages/vue-vscode/node_modules/typescript/lib`,
 * where `extensionPath` resolves to the `packages/vue-vscode` package and its
 * pinned `typescript` dependency lives. The bare repo root is NOT the analog: a
 * pnpm workspace leaves no hoisted `typescript` there, so a bare-root tsdk would
 * point `expectedTsserverJs` at an absent file and C's strict existence gate would
 * reject the run.
 *
 * Every emitted path is canonicalised (forward slashes, lowercase drive letter)
 * with {@link canonicalizePath} so it compares equal to the value C derives, and
 * paths are composed with {@link joinCanonical} over canonical inputs (which
 * preserves a UNC `//` prefix) rather than hardcoded separators — portable
 * across macOS, Windows, and Linux.
 */

import { readFileSync } from "node:fs";

import { canonicalizePath, joinCanonical } from "./paths.js";

/** The resolved, pinned TypeScript tool root the baseline runs against. */
export interface ToolRoots {
  /** Canonical workspace/repository root. */
  repoRoot: string;
  /** Canonical tsdk directory (`…/typescript/lib`) passed to the provider. */
  tsserverTsdk: string;
  /** Canonical `…/typescript/lib/tsserver.js` the bridge enforces a match against. */
  expectedTsserverJs: string;
  /** Pinned TypeScript version (from `…/typescript/package.json`), when readable. */
  tsserverVersion?: string;
  /** Optional pinned `tsgo` binary (strict tsgo runs require it). */
  tsgoBin?: string;
}

/** Options for {@link resolveToolRoots}. */
export interface ResolveToolRootsOptions {
  /**
   * Explicit tsdk override — the harness analog of the `verter.typescript.tsdk`
   * user setting. When set it wins over the repo-bundled default.
   */
  userTsdk?: string;
  /** Optional pinned `tsgo` binary path. */
  tsgoBin?: string;
  /**
   * Reads the pinned TypeScript version for a resolved tsdk. Injectable so the
   * resolver stays pure under test; defaults to {@link readTypescriptVersionFromDisk}.
   */
  readTypescriptVersion?: (tsdk: string) => string | undefined;
}

/**
 * Read the pinned TypeScript version from `<tsdk>/../package.json`
 * (`…/typescript/package.json`). Returns `undefined` when the file is absent,
 * unreadable, unparseable, or carries no string `version`.
 */
export function readTypescriptVersionFromDisk(tsdk: string): string | undefined {
  const pkgPath = joinCanonical(canonicalizePath(tsdk), "..", "package.json");
  try {
    const parsed: unknown = JSON.parse(readFileSync(pkgPath, "utf-8"));
    if (parsed && typeof parsed === "object" && "version" in parsed) {
      const version = (parsed as { version: unknown }).version;
      return typeof version === "string" ? version : undefined;
    }
  } catch {
    // Absent/unreadable/unparseable → treated as "no pinned version".
  }
  return undefined;
}

/**
 * The TypeScript lib the shipped extension passes as its `--tsdk` fallback:
 * `<repoRoot>/packages/vue-vscode/node_modules/typescript/lib`.
 *
 * This mirrors `buildServerOptions` in `packages/vue-vscode/src/extension.ts`
 * (`join(extensionPath, "node_modules", "typescript", "lib")`), where
 * `extensionPath` is the `packages/vue-vscode` package and its pinned
 * `typescript` dependency is installed. Unlike the bare repo root — which a pnpm
 * workspace leaves without a hoisted `typescript` — this path resolves to a real
 * `tsserver.js`, so the pinned tool root C enforces actually exists on disk.
 */
function bundledExtensionTsdk(repoRootCanon: string): string {
  return canonicalizePath(
    joinCanonical(repoRootCanon, "packages", "vue-vscode", "node_modules", "typescript", "lib"),
  );
}

/**
 * Resolve the deterministic tool roots for a repository.
 *
 * The tsdk is the explicit `userTsdk` when supplied, otherwise the bundled
 * TypeScript the shipped extension ships
 * ({@link bundledExtensionTsdk} — `<repoRoot>/packages/vue-vscode/node_modules/typescript/lib`).
 * `expectedTsserverJs` is always `<tsdk>/tsserver.js`, so the bridge's enforced
 * match is consistent with the tsdk it discovers against.
 */
export function resolveToolRoots(repoRoot: string, opts: ResolveToolRootsOptions = {}): ToolRoots {
  const repoRootCanon = canonicalizePath(repoRoot);
  const tsserverTsdk = opts.userTsdk
    ? canonicalizePath(opts.userTsdk)
    : bundledExtensionTsdk(repoRootCanon);
  const expectedTsserverJs = canonicalizePath(joinCanonical(tsserverTsdk, "tsserver.js"));
  const readVersion = opts.readTypescriptVersion ?? readTypescriptVersionFromDisk;

  return {
    repoRoot: repoRootCanon,
    tsserverTsdk,
    expectedTsserverJs,
    tsserverVersion: readVersion(tsserverTsdk),
    tsgoBin: opts.tsgoBin ? canonicalizePath(opts.tsgoBin) : undefined,
  };
}
