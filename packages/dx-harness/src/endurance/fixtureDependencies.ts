/** @ai-generated - Materializes the explicit declaration-only fixture dependency closure. */
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type { EnduranceFramework } from "./types.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..", "..", "..", "..");
const DECLARATION_FILE = /\.d\.(?:ts|mts|cts)$/;

export type EnduranceFixtureFramework = EnduranceFramework;

interface EnduranceFixtureDependency {
  readonly packageName: string;
  readonly source:
    | { readonly kind: "repository"; readonly segments: readonly string[] }
    | { readonly kind: "svelte-install-peer" };
}

const INSTALLED_SVELTE = ["packages", "svelte-jsx", "node_modules", "svelte"] as const;

/** Installed type packages required by each synthetic endurance framework. */
export const ENDURANCE_FIXTURE_DEPENDENCIES: Readonly<
  Record<EnduranceFixtureFramework, readonly EnduranceFixtureDependency[]>
> = {
  vue: [],
  svelte: [
    {
      packageName: "svelte",
      source: { kind: "repository", segments: INSTALLED_SVELTE },
    },
    { packageName: "esrap", source: { kind: "svelte-install-peer" } },
    { packageName: "clsx", source: { kind: "svelte-install-peer" } },
    { packageName: "magic-string", source: { kind: "svelte-install-peer" } },
    { packageName: "@types/estree", source: { kind: "svelte-install-peer" } },
    { packageName: "locate-character", source: { kind: "svelte-install-peer" } },
    {
      packageName: "@jridgewell/sourcemap-codec",
      source: { kind: "svelte-install-peer" },
    },
  ],
};

function fixtureDependencySourceRoot(dependency: EnduranceFixtureDependency): string {
  if (dependency.source.kind === "repository") {
    return realpathSync(path.join(REPO_ROOT, ...dependency.source.segments));
  }
  // pnpm installs Svelte's declared dependency links beside the canonical
  // Svelte package in its isolated `node_modules`. Resolve through that
  // installation rather than ambient repository hoisting.
  const svelteRoot = realpathSync(path.join(REPO_ROOT, ...INSTALLED_SVELTE));
  return realpathSync(path.join(path.dirname(svelteRoot), ...dependency.packageName.split("/")));
}

function isWithin(parent: string, candidate: string): boolean {
  const relative = path.relative(parent, candidate);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

function assertOwnedSyntheticRoot(root: string): string {
  const resolved = path.resolve(root);
  if (
    path.dirname(resolved) !== path.resolve(tmpdir()) ||
    !path.basename(resolved).startsWith("verter-endurance-")
  ) {
    throw new Error(`fixture dependencies require an owned synthetic endurance root: ${resolved}`);
  }
  return resolved;
}

function declarationExportEntrypoints(manifest: unknown): string[] {
  if (manifest === null || typeof manifest !== "object") return [];
  const packageJson = manifest as { types?: unknown; exports?: unknown };
  const entries: string[] = [];
  if (typeof packageJson.types === "string") entries.push(packageJson.types);
  const visit = (value: unknown, key?: string) => {
    if (key === "types" && typeof value === "string") {
      entries.push(value);
      return;
    }
    if (value === null || typeof value !== "object") return;
    for (const [childKey, childValue] of Object.entries(value)) visit(childValue, childKey);
  };
  visit(packageJson.exports);
  return [...new Set(entries)];
}

function copyDeclarationTree(sourceRoot: string, destinationRoot: string): void {
  const visit = (sourceDirectory: string) => {
    for (const entry of readdirSync(sourceDirectory, { withFileTypes: true })) {
      if (entry.name === "node_modules") continue;
      const source = path.join(sourceDirectory, entry.name);
      if (entry.isSymbolicLink()) {
        throw new Error(`fixture dependency source contains an unsupported symlink: ${source}`);
      }
      if (entry.isDirectory()) {
        visit(source);
        continue;
      }
      if (!entry.isFile() || !DECLARATION_FILE.test(entry.name)) continue;
      const destination = path.join(destinationRoot, path.relative(sourceRoot, source));
      mkdirSync(path.dirname(destination), { recursive: true });
      copyFileSync(source, destination);
    }
  };
  visit(sourceRoot);
}

/** Stage installed declarations into an owned synthetic workspace. */
export function stageEnduranceFixtureDependencies(
  root: string,
  framework: EnduranceFixtureFramework,
): void {
  const ownedRoot = assertOwnedSyntheticRoot(root);
  for (const dependency of ENDURANCE_FIXTURE_DEPENDENCIES[framework]) {
    const sourceRoot = fixtureDependencySourceRoot(dependency);
    const sourceManifestPath = path.join(sourceRoot, "package.json");
    const manifestBytes = readFileSync(sourceManifestPath);
    const manifest = JSON.parse(manifestBytes.toString("utf8")) as { name?: unknown };
    if (manifest.name !== dependency.packageName) {
      throw new Error(
        `fixture dependency manifest expected ${dependency.packageName}, got ${String(manifest.name)}`,
      );
    }

    const destinationRoot = path.join(
      ownedRoot,
      "node_modules",
      ...dependency.packageName.split("/"),
    );
    if (!isWithin(ownedRoot, destinationRoot)) {
      throw new Error(`fixture dependency destination escaped its owned root: ${destinationRoot}`);
    }
    if (existsSync(destinationRoot)) {
      throw new Error(`fixture dependency destination already exists: ${destinationRoot}`);
    }
    mkdirSync(destinationRoot, { recursive: true });
    copyFileSync(sourceManifestPath, path.join(destinationRoot, "package.json"));
    copyDeclarationTree(sourceRoot, destinationRoot);

    for (const entrypoint of declarationExportEntrypoints(manifest)) {
      const destination = path.resolve(destinationRoot, entrypoint);
      if (!isWithin(destinationRoot, destination) || !DECLARATION_FILE.test(destination)) {
        throw new Error(`fixture dependency has an invalid declaration entrypoint: ${entrypoint}`);
      }
      if (!existsSync(destination) || lstatSync(destination).isSymbolicLink()) {
        throw new Error(`fixture dependency declaration entrypoint was not copied: ${destination}`);
      }
    }
  }
}
