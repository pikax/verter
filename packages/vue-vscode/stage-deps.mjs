import { createRequire } from "node:module";
import {
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const DEPENDENCY_FIELDS = ["dependencies", "optionalDependencies", "peerDependencies"];

/**
 * Replace workspace protocol ranges with concrete versions.
 *
 * Each workspace dependency resolves to ITS OWN version, read from that package
 * in the workspace. The depending package's version is only a fallback for a
 * name that is not a workspace package.
 *
 * The extension version is deliberately NOT usable as a blanket substitute: the
 * VSIX version follows the VS Code Marketplace's plain-semver rule (no
 * prerelease suffix), while the workspace packages carry the monorepo's npm
 * version (e.g. `0.0.1-beta.2`). Those two are independent, and pinning a
 * workspace range to the extension version makes vsce's production-tree check
 * fail with npm ELSPROBLEMS `invalid: <pkg>@<real-version>`.
 */
export function patchWorkspaceRanges(manifest, fallbackVersion, workspacePackages) {
  for (const field of DEPENDENCY_FIELDS) {
    const dependencies = manifest[field];
    if (!dependencies) continue;
    for (const [name, range] of Object.entries(dependencies)) {
      if (typeof range === "string" && range.startsWith("workspace:")) {
        dependencies[name] = workspaceVersionOf(name, workspacePackages) ?? fallbackVersion;
      }
    }
  }
  return manifest;
}

/** The on-disk version of a workspace package, or null when it is not one. */
function workspaceVersionOf(name, workspacePackages) {
  const packageDir = workspacePackages?.get(name);
  if (!packageDir) return null;
  const version = readManifest(packageDir).version;
  return typeof version === "string" ? version : null;
}

/**
 * Materialize an npm-compatible production dependency tree for VSCE.
 *
 * pnpm's workspace dependencies are junctions. VSCE asks npm to validate the
 * production tree, and npm treats those junction targets as workspace roots,
 * incorrectly validating their development dependencies. Copying each package
 * as real files gives npm the same layout users receive from a published VSIX.
 * Dependencies are kept below their owning package, so incompatible versions
 * cannot be accidentally flattened together.
 */
export function stageRuntimeDependencies({
  packageDir,
  workspaceRoot,
  destinationNodeModules,
  packageVersion,
}) {
  const workspacePackages = discoverWorkspacePackages(workspaceRoot);
  const rootManifest = readManifest(packageDir);
  const staged = new Set();
  mkdirSync(destinationNodeModules, { recursive: true });

  for (const name of Object.keys(rootManifest.dependencies ?? {})) {
    const sourceDir = resolvePackageDir(name, packageDir, workspacePackages);
    stagePackage({
      name,
      sourceDir,
      installRoot: destinationNodeModules,
      workspacePackages,
      packageVersion,
      staged,
    });
  }
}

function stagePackage({ name, sourceDir, installRoot, workspacePackages, packageVersion, staged }) {
  const destination = packageDestination(installRoot, name);
  const stageKey = `${path.resolve(destination)}\0${name}`;
  if (staged.has(stageKey)) return;
  staged.add(stageKey);

  assertWithin(installRoot, destination);
  removePath(destination);

  const isWorkspacePackage = workspacePackages.get(name) === path.resolve(sourceDir);
  const sourceManifest = readManifest(sourceDir);
  if (isWorkspacePackage) {
    copyWorkspacePackage(sourceDir, destination, sourceManifest);
  } else {
    copyRegistryPackage(sourceDir, destination);
  }

  const stagedManifest = readManifest(destination);
  if (isWorkspacePackage) {
    patchWorkspaceRanges(stagedManifest, packageVersion, workspacePackages);
    writeFileSync(
      path.join(destination, "package.json"),
      `${JSON.stringify(stagedManifest, null, 2)}\n`,
    );
  }

  const nestedModules = path.join(destination, "node_modules");
  for (const dependencyName of Object.keys(stagedManifest.dependencies ?? {})) {
    const dependencySource = resolvePackageDir(dependencyName, sourceDir, workspacePackages);
    stagePackage({
      name: dependencyName,
      sourceDir: dependencySource,
      installRoot: nestedModules,
      workspacePackages,
      packageVersion,
      staged,
    });
  }

  for (const peerName of Object.keys(stagedManifest.peerDependencies ?? {})) {
    if (stagedManifest.peerDependenciesMeta?.[peerName]?.optional) continue;
    const peerSource = resolvePackageDir(peerName, sourceDir, workspacePackages);
    stagePackage({
      name: peerName,
      sourceDir: peerSource,
      installRoot,
      workspacePackages,
      packageVersion,
      staged,
    });
  }
}

/** Map every workspace package name to its directory. Discover once, reuse. */
export function discoverWorkspacePackages(workspaceRoot) {
  const packages = new Map();
  const packagesDir = path.join(workspaceRoot, "packages");
  for (const entry of readdirSync(packagesDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const packageDir = path.join(packagesDir, entry.name);
    const manifestPath = path.join(packageDir, "package.json");
    if (!existsSync(manifestPath)) continue;
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    if (typeof manifest.name === "string") {
      packages.set(manifest.name, path.resolve(packageDir));
    }
  }
  return packages;
}

function resolvePackageDir(name, fromDir, workspacePackages) {
  const workspacePackage = workspacePackages.get(name);
  if (workspacePackage) return workspacePackage;

  const installedPackage = findInstalledPackageDir(name, fromDir);
  if (installedPackage) return installedPackage;

  const require = createRequire(path.join(fromDir, "package.json"));
  try {
    return path.dirname(realpathSync(require.resolve(`${name}/package.json`)));
  } catch (packageJsonError) {
    try {
      return findOwningPackageDir(require.resolve(name), name);
    } catch {
      throw new Error(`Cannot resolve production dependency ${name} from ${fromDir}`, {
        cause: packageJsonError,
      });
    }
  }
}

function findInstalledPackageDir(name, fromDir) {
  let current = path.resolve(fromDir);
  for (;;) {
    const candidate = path.join(current, "node_modules", ...name.split("/"));
    if (existsSync(path.join(candidate, "package.json"))) {
      return realpathSync(candidate);
    }
    const parent = path.dirname(current);
    if (parent === current) return null;
    current = parent;
  }
}

function findOwningPackageDir(resolvedEntry, expectedName) {
  let current = path.dirname(realpathSync(resolvedEntry));
  for (;;) {
    const manifestPath = path.join(current, "package.json");
    if (existsSync(manifestPath)) {
      const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
      if (manifest.name === expectedName) return current;
    }
    const parent = path.dirname(current);
    if (parent === current) break;
    current = parent;
  }
  throw new Error(`Resolved ${expectedName} entry has no matching package root: ${resolvedEntry}`);
}

function copyWorkspacePackage(sourceDir, destination, manifest) {
  mkdirSync(destination, { recursive: true });
  copyEntry(path.join(sourceDir, "package.json"), path.join(destination, "package.json"));

  for (const relative of manifest.files ?? []) {
    const source = path.join(sourceDir, relative);
    if (!existsSync(source)) continue;
    copyEntry(source, path.join(destination, relative));
  }

  // Native bindings are staged directly for the VSIX target. They are not in
  // @verter/native's npm `files` list because normal npm publication supplies
  // them through platform packages.
  if (manifest.napi && existsSync(path.join(sourceDir, "dist"))) {
    for (const entry of readdirSync(path.join(sourceDir, "dist"), { withFileTypes: true })) {
      if (entry.isFile() && entry.name.endsWith(".node") && !entry.name.endsWith(".old.node")) {
        copyEntry(
          path.join(sourceDir, "dist", entry.name),
          path.join(destination, "dist", entry.name),
        );
      }
    }
  }
}

function copyRegistryPackage(sourceDir, destination) {
  cpSync(realpathSync(sourceDir), destination, {
    recursive: true,
    filter(source) {
      const relative = path.relative(sourceDir, source);
      if (!relative) return true;
      const firstSegment = relative.split(path.sep)[0];
      return firstSegment !== "node_modules" && firstSegment !== ".git";
    },
  });
}

function copyEntry(source, destination) {
  mkdirSync(path.dirname(destination), { recursive: true });
  cpSync(source, destination, { recursive: lstatSync(source).isDirectory() });
}

function readManifest(packageDir) {
  return JSON.parse(readFileSync(path.join(packageDir, "package.json"), "utf8"));
}

function packageDestination(installRoot, name) {
  const segments = name.split("/");
  if (segments.some((segment) => !segment || segment === "." || segment === "..")) {
    throw new Error(`Invalid package name in production dependency graph: ${name}`);
  }
  return path.join(installRoot, ...segments);
}

function assertWithin(root, candidate) {
  const relative = path.relative(path.resolve(root), path.resolve(candidate));
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`Refusing to stage dependency outside node_modules: ${candidate}`);
  }
}

function removePath(target) {
  if (!existsSync(target) && !safeLstat(target)) return;
  const stat = safeLstat(target);
  if (stat?.isSymbolicLink()) {
    rmSync(target);
  } else {
    rmSync(target, { recursive: true, force: true });
  }
}

function safeLstat(target) {
  try {
    return lstatSync(target);
  } catch {
    return null;
  }
}
