// Transitive package-closure enumeration and comparison.
//
// Two independent enumerations of the pinned oracle dependency closure are
// compared against each other and against the committed evidence:
//
//  1. LOCK-DERIVED: every entry of the committed npm `package-lock.json` —
//     exact package paths, names, versions, integrity hashes, resolution
//     URLs, and per-package dependency edges (each edge resolved through
//     npm's nested node_modules resolution walk). The derivation is
//     byte-compatible with the committed `closure.tsv` evidence format, so
//     the two can be compared as text.
//
//  2. REALIZED: a filesystem walk of an actually-installed node_modules
//     tree (the disposable, scripts-disabled install a self-test produces
//     from the committed lockfile) — every installed package's real
//     manifest name/version at its real nested path, plus its dependency
//     edges re-resolved against the physical tree.
//
// A transitive mutation anywhere — a nested lock entry's version or
// integrity, a tampered installed package, a closure.tsv edit — breaks at
// least one comparison. Nothing here trusts a single artifact's
// self-description.

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";

export class ClosureDriftError extends Error {
  constructor(message, details) {
    super(message);
    this.name = "ClosureDriftError";
    this.details = details;
  }
}

function packageName(entryPath, entry) {
  return (
    entry.name ?? entryPath.slice(entryPath.lastIndexOf("node_modules/") + "node_modules/".length)
  );
}

function resolveDependency(packages, parentPath, name) {
  let base = parentPath;
  while (true) {
    const candidate = base ? `${base}/node_modules/${name}` : `node_modules/${name}`;
    if (packages[candidate]) return packages[candidate].version;
    const marker = base.lastIndexOf("/node_modules/");
    if (marker < 0) break;
    base = base.slice(0, marker);
  }
  return packages[`node_modules/${name}`]?.version;
}

function dependencyField(packages, entryPath, entry, field, optionalPeer = false) {
  const source = entry[field] ?? {};
  const resolved = Object.keys(source)
    .sort()
    .map((name) => {
      const version = resolveDependency(packages, entryPath, name);
      if (version) return `${name}@${version}`;
      if (optionalPeer && entry.peerDependenciesMeta?.[name]?.optional)
        return `${name}=OMITTED_OPTIONAL_PEER`;
      throw new ClosureDriftError(`${entryPath}: unresolved ${field} dependency ${name}`, {
        entryPath,
        field,
        name,
      });
    })
    .join(",");
  return resolved === "" ? "-" : resolved;
}

export const CLOSURE_COLUMNS = [
  "path",
  "name",
  "version",
  "integrity",
  "resolved",
  "direct",
  "dependencies",
  "optional_dependencies",
  "peer_dependencies",
];

/**
 * Enumeration 1: the committed lockfile's full transitive closure, one row
 * per installed package path, in the committed closure.tsv row format.
 *
 * @returns {Array<object>} rows sorted by (name, path)
 */
export function enumerateLockClosure(lockPath) {
  const lock = JSON.parse(readFileSync(lockPath, "utf8"));
  const direct = new Set(Object.keys(lock.packages?.[""]?.dependencies ?? {}));
  const rows = [];
  for (const [entryPath, entry] of Object.entries(lock.packages ?? {})) {
    if (entryPath === "") continue;
    const name = packageName(entryPath, entry);
    rows.push({
      path: entryPath,
      name,
      version: entry.version,
      integrity: entry.integrity ?? "-",
      resolved: entry.resolved ?? "-",
      direct: direct.has(name) ? "yes" : "no",
      dependencies: dependencyField(lock.packages, entryPath, entry, "dependencies"),
      optional_dependencies: dependencyField(
        lock.packages,
        entryPath,
        entry,
        "optionalDependencies",
      ),
      peer_dependencies: dependencyField(lock.packages, entryPath, entry, "peerDependencies", true),
    });
  }
  rows.sort((a, b) => a.name.localeCompare(b.name) || a.path.localeCompare(b.path));
  return rows;
}

export function closureRowsToTsv(rows) {
  return (
    [
      CLOSURE_COLUMNS.join("\t"),
      ...rows.map((row) => CLOSURE_COLUMNS.map((column) => row[column]).join("\t")),
    ].join("\n") + "\n"
  );
}

/**
 * Canonical digest of a closure enumeration (order-independent input).
 * Rows carrying a `contentSha256` (realized-tree enumerations) contribute
 * it to the digest, so a file-content change anywhere in an installed
 * closure changes the overall digest; lock-derived and committed-TSV rows
 * have no content column and digest exactly as before.
 */
export function closureDigest(rows) {
  const canonical = rows
    .map((row) => {
      const base = CLOSURE_COLUMNS.map((column) => row[column] ?? "-").join("\t");
      return row.contentSha256 === undefined ? base : `${base}\t${row.contentSha256}`;
    })
    .sort()
    .join("\n");
  return createHash("sha256").update(canonical, "utf8").digest("hex");
}

/**
 * Deterministic content digest of ONE installed package directory. Every
 * regular file under the package — EXCLUDING nested `node_modules`, whose
 * packages are enumerated as their own closure rows — is listed
 * recursively, sorted by its `/`-joined relative path (plain code-point
 * order, locale-independent), and folded into one SHA-256 as
 * `relativePath + "\0" + fileBytes + "\0"` per file. File bytes enter raw
 * (no EOL normalization): installed package payloads are byte-exact npm
 * artifacts, not checked-out text. A non-regular directory entry (e.g. a
 * symlink) contributes no bytes, but replacing a regular file WITH one
 * still changes the digest because the file leaves the sorted list.
 */
function packageContentSha256(dir) {
  const files = [];
  (function walkFiles(current, relPrefix) {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      if (entry.name === "node_modules") continue;
      const rel = relPrefix === "" ? entry.name : `${relPrefix}/${entry.name}`;
      if (entry.isDirectory()) walkFiles(path.join(current, entry.name), rel);
      else if (entry.isFile()) files.push(rel);
    }
  })(dir, "");
  files.sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const hash = createHash("sha256");
  for (const rel of files) {
    hash.update(rel, "utf8");
    hash.update("\0", "utf8");
    hash.update(readFileSync(path.join(dir, ...rel.split("/"))));
    hash.update("\0", "utf8");
  }
  return hash.digest("hex");
}

/**
 * Enumeration 2: walk a REAL installed node_modules tree. Every directory
 * holding a package.json is recorded at its npm-style relative path with
 * the name/version its own manifest declares, PLUS a `contentSha256` over
 * the package directory's full file contents (packageContentSha256 above)
 * — so a tampered payload file whose package.json name/version is left
 * untouched still changes the enumeration and every digest folded from it.
 *
 * @returns {Array<{ path: string, name: string, version: string,
 *   contentSha256: string, manifest: object }>}
 */
export function enumerateInstalledClosure(installRoot) {
  const results = [];
  function walk(nodeModulesDir, prefix) {
    if (!existsSync(nodeModulesDir)) return;
    for (const entry of readdirSync(nodeModulesDir, { withFileTypes: true })) {
      if (!entry.isDirectory() && !entry.isSymbolicLink()) continue;
      if (entry.name === ".bin" || entry.name === ".cache") continue;
      if (entry.name.startsWith("@")) {
        const scopeDir = path.join(nodeModulesDir, entry.name);
        for (const scoped of readdirSync(scopeDir, { withFileTypes: true })) {
          if (!scoped.isDirectory() && !scoped.isSymbolicLink()) continue;
          visitPackage(
            path.join(scopeDir, scoped.name),
            `${prefix}node_modules/${entry.name}/${scoped.name}`,
          );
        }
      } else {
        visitPackage(path.join(nodeModulesDir, entry.name), `${prefix}node_modules/${entry.name}`);
      }
    }
  }
  function visitPackage(dir, relPath) {
    const manifestPath = path.join(dir, "package.json");
    if (existsSync(manifestPath)) {
      const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
      results.push({
        path: relPath,
        name: manifest.name,
        version: manifest.version,
        contentSha256: packageContentSha256(dir),
        manifest,
      });
    }
    walk(path.join(dir, "node_modules"), `${relPath}/`);
  }
  walk(path.join(installRoot, "node_modules"), "");
  results.sort((a, b) => a.path.localeCompare(b.path));
  return results;
}

/**
 * Compares the realized installed tree against the lock-derived closure:
 * exact path set, exact name@version per path, and every dependency edge
 * re-resolved through the PHYSICAL tree matching the lock's recorded edge
 * resolution. Any deviation — an extra package, a missing one, a version
 * substituted at any depth, an edge resolving differently — is reported.
 *
 * @returns {{ ok: boolean, problems: string[] }}
 */
export function compareRealizedToLock(realized, lockRows) {
  const problems = [];
  const realizedByPath = new Map(realized.map((entry) => [entry.path, entry]));
  const lockByPath = new Map(lockRows.map((row) => [row.path, row]));

  for (const row of lockRows) {
    const installed = realizedByPath.get(row.path);
    if (installed === undefined) {
      problems.push(`missing installed package: ${row.path} (${row.name}@${row.version})`);
      continue;
    }
    if (installed.name !== row.name || installed.version !== row.version) {
      problems.push(
        `${row.path}: installed ${installed.name}@${installed.version}, lock records ${row.name}@${row.version}`,
      );
    }
  }
  for (const entry of realized) {
    if (!lockByPath.has(entry.path)) {
      problems.push(
        `extra installed package not in lock: ${entry.path} (${entry.name}@${entry.version})`,
      );
    }
  }

  // Edge check: re-resolve each package's declared dependencies against the
  // realized tree exactly as Node/npm would (nearest nested, walking up).
  const realizedVersions = new Map(realized.map((entry) => [entry.path, entry]));
  function resolveInTree(fromPath, depName) {
    let base = fromPath;
    while (true) {
      const candidate = `${base}/node_modules/${depName}`;
      if (realizedVersions.has(candidate)) return realizedVersions.get(candidate).version;
      const marker = base.lastIndexOf("/node_modules/");
      if (marker < 0) break;
      base = base.slice(0, marker);
    }
    return realizedVersions.get(`node_modules/${depName}`)?.version;
  }
  for (const row of lockRows) {
    if (row.dependencies === "-" || row.dependencies === undefined) continue;
    const installed = realizedByPath.get(row.path);
    if (installed === undefined) continue; // already reported
    for (const edge of row.dependencies.split(",")) {
      const at = edge.lastIndexOf("@");
      const depName = edge.slice(0, at);
      const depVersion = edge.slice(at + 1);
      const realizedVersion = resolveInTree(row.path, depName);
      if (realizedVersion !== depVersion) {
        problems.push(
          `${row.path}: dependency ${depName} realizes ${realizedVersion ?? "UNRESOLVED"}, lock edge records ${depVersion}`,
        );
      }
    }
  }

  return { ok: problems.length === 0, problems };
}
