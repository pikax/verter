/**
 * Committed vendored Vue shim source plus a content manifest.
 *
 * The differential baseline is hermetic: it must type-check generated `.vue.tsx`
 * without a runtime `npm`/`pnpm` install. The `vue` / `@vue/*` declarations live
 * as committed shims under `vendor/shims/`, pinned to the workspace Vue
 * line ({@link VENDORED_VUE_VERSION}). The `verter_dx_baseline` materializer
 * copies them into the baseline root and, in strict CI, refuses any vendored
 * `vue`/`@vue/*` whose version differs from the `expectedVueVersion` B computes
 * here — so the baseline can never silently run against a Vue line different from
 * the one it claims.
 *
 * `expectedVueVersion` is computed ONCE from the committed `vue/package.json`
 * (the single source of truth); a content manifest over the whole vendor tree
 * gives a tamper-evident inventory.
 */

import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { posix } from "node:path";
import { fileURLToPath } from "node:url";

import { canonicalizePath, joinCanonical } from "./paths.js";

/**
 * The pinned Vue line for the vendored shims — the workspace `vue` /
 * `@vue/compiler-*` line (`^3.5.34`). Every committed vendored `package.json`
 * carries exactly this version; the {@link vendorManifest} test asserts it.
 */
export const VENDORED_VUE_VERSION = "3.5.34";

/**
 * Canonical absolute path to the committed `vendor/shims` directory — the
 * vendored `node_modules` overlay C copies into the baseline root. It is named
 * `shims` (not `node_modules`) so the repo-wide `node_modules` gitignore rule
 * does not exclude the committed declarations.
 */
export function vendorShimsDir(): string {
  // `import.meta.url` is this module's file URL under both `src` (vitest) and the
  // emitted `dist` — both sit one level under the package root, so the vendor
  // tree is a sibling `../vendor/shims`.
  return canonicalizePath(fileURLToPath(new URL("../vendor/shims", import.meta.url)));
}

/** Lowercase hex SHA-256 of a byte buffer. */
export function sha256Hex(bytes: Buffer): string {
  return createHash("sha256").update(bytes).digest("hex");
}

/**
 * Read the pinned Vue version from the committed `vue/package.json` — the single
 * source from which `expectedVueVersion` is derived.
 */
export function computeExpectedVueVersion(vendorRoot: string = vendorShimsDir()): string {
  const pkg = readPackageJson(joinCanonical(vendorRoot, "vue", "package.json"));
  const version = pkg?.version;
  if (typeof version !== "string") {
    throw new Error(`vendored vue/package.json has no string version at ${vendorRoot}`);
  }
  return version;
}

/** One vendored Vue package and its pinned version. */
export interface VuePackageVersion {
  package: string;
  version: string;
}

/**
 * Collect `(package, version)` for every vendored Vue package the version-sync
 * contract covers: the `vue` core plus every `@vue/*` scope package present.
 * Deterministically ordered (`vue` first, then `@vue/*` lexicographically) so a
 * mismatch is reported against a stable first package.
 */
export function collectVuePackageVersions(
  vendorRoot: string = vendorShimsDir(),
): VuePackageVersion[] {
  const out: VuePackageVersion[] = [{ package: "vue", version: requireVersion(vendorRoot, "vue") }];
  const scope = joinCanonical(vendorRoot, "@vue");
  for (const name of safeReaddir(scope)) {
    out.push({ package: `@vue/${name}`, version: requireVersion(vendorRoot, `@vue/${name}`) });
  }
  return out;
}

/** A vendored file and its checksum. */
export interface VendorFile {
  /** Path relative to the vendor root, forward-slashed. */
  path: string;
  /** Byte length. */
  bytes: number;
  /** Lowercase hex SHA-256 of the file bytes. */
  sha256: string;
}

/** A tamper-evident inventory of the vendored shim source. */
export interface VendorManifest {
  /** The pinned Vue line every vendored package is expected to carry. */
  vueVersion: string;
  /** Every vendored file, sorted by path. */
  files: VendorFile[];
}

/** Build the content manifest over the committed vendored shim tree. */
export function buildVendorManifest(vendorRoot: string = vendorShimsDir()): VendorManifest {
  const relPaths: string[] = [];
  walk(vendorRoot, "", relPaths);
  relPaths.sort();
  const files: VendorFile[] = relPaths.map((rel) => {
    const bytes = readFileSync(joinCanonical(vendorRoot, rel));
    return { path: rel, bytes: bytes.byteLength, sha256: sha256Hex(bytes) };
  });
  return { vueVersion: computeExpectedVueVersion(vendorRoot), files };
}

// ── internals ──────────────────────────────────────────────────────────────

interface PackageJson {
  version?: unknown;
}

function readPackageJson(path: string): PackageJson | undefined {
  try {
    return JSON.parse(readFileSync(path, "utf-8")) as PackageJson;
  } catch {
    return undefined;
  }
}

function requireVersion(vendorRoot: string, pkg: string): string {
  const json = readPackageJson(joinCanonical(vendorRoot, pkg, "package.json"));
  return typeof json?.version === "string" ? json.version : "<absent>";
}

function safeReaddir(dir: string): string[] {
  try {
    return readdirSync(dir, { withFileTypes: true })
      .filter((e) => e.isDirectory())
      .map((e) => e.name)
      .sort();
  } catch {
    return [];
  }
}

function walk(root: string, rel: string, out: string[]): void {
  // `root` is a canonical (possibly UNC) base; `rel`/`childRel` are forward-slashed
  // relative segments, so they keep the plain posix join.
  const here = rel === "" ? root : joinCanonical(root, rel);
  for (const entry of readdirSync(here, { withFileTypes: true })) {
    const childRel = rel === "" ? entry.name : posix.join(rel, entry.name);
    if (entry.isDirectory()) {
      walk(root, childRel, out);
    } else if (entry.isFile()) {
      out.push(childRel);
    }
  }
}
