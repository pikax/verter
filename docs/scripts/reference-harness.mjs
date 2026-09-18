#!/usr/bin/env node
/**
 * Docs reference harness: offline pinned example tests, generated-reference
 * consistency, link and capability checks. Resolves Verter imports against
 * published package.json exports and commands against shipped package bins.
 *
 * Usage: node docs/scripts/reference-harness.mjs
 */

import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawn } from "node:child_process";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_REPO_ROOT = resolve(SCRIPT_DIR, "..", "..");

const SPECIFIER_RE = /(?:from\s+|import\s*\(\s*|require\s*\(\s*)["']([^"']+)["']/g;
const MD_LINK_RE = /\[[^\]]*]\(([^)]+)\)/g;
const INTERNAL_DIR_RE = /^(packages\/[^/]+\/src\/|crates\/|scripts\/)/;

export const HARNESS_ID = "docs-reference-harness";

export function posixRel(from, to) {
  return relative(from, to).split(sep).join("/");
}

export function sha256Text(text) {
  return createHash("sha256").update(text.replace(/\r\n/g, "\n"), "utf8").digest("hex");
}

export function splitPackageSpecifier(specifier) {
  if (specifier.startsWith("node:") || specifier.startsWith("file:")) {
    return { kind: specifier.startsWith("node:") ? "node" : "file", name: specifier, subpath: "" };
  }
  if (specifier.startsWith(".")) return { kind: "relative", name: specifier, subpath: "" };
  if (specifier.startsWith("@")) {
    const parts = specifier.split("/");
    return { kind: "package", name: `${parts[0]}/${parts[1]}`, subpath: parts.slice(2).join("/") };
  }
  const slash = specifier.indexOf("/");
  if (slash === -1) return { kind: "package", name: specifier, subpath: "" };
  return {
    kind: "package",
    name: specifier.slice(0, slash),
    subpath: specifier.slice(slash + 1),
  };
}

function exportEntry(exportsField, subpath) {
  if (exportsField == null) return null;
  const key = subpath ? `./${subpath}` : ".";
  if (typeof exportsField === "string") return subpath ? null : exportsField;
  const entry = exportsField[key];
  if (entry == null) return null;
  if (typeof entry === "string") return entry;
  return entry.import ?? entry.default ?? entry.require ?? entry.types ?? null;
}

function walkPackageJsonFiles(repoRoot) {
  const roots = [join(repoRoot, "packages")];
  const files = [];
  for (const root of roots) {
    if (!existsSync(root)) continue;
    for (const name of readdirSync(root)) {
      const dir = join(root, name);
      if (!statSync(dir).isDirectory()) continue;
      const pkg = join(dir, "package.json");
      if (existsSync(pkg)) files.push(pkg);
      const npm = join(dir, "npm");
      if (existsSync(npm) && statSync(npm).isDirectory()) {
        for (const nested of readdirSync(npm)) {
          const nestedPkg = join(npm, nested, "package.json");
          if (existsSync(nestedPkg)) files.push(nestedPkg);
        }
      }
    }
  }
  return files.sort();
}

export function loadWorkspacePackages(repoRoot, readFile = defaultRead) {
  const byName = new Map();
  for (const file of walkPackageJsonFiles(repoRoot)) {
    const pkg = JSON.parse(readFile(file));
    if (!pkg.name) continue;
    const dir = dirname(file);
    byName.set(pkg.name, {
      name: pkg.name,
      dir,
      relDir: posixRel(repoRoot, dir),
      private: pkg.private === true,
      version: pkg.version ?? null,
      exports: pkg.exports ?? null,
      bin: pkg.bin ?? null,
      main: pkg.main ?? null,
    });
  }
  return byName;
}

export function resolvePackageExport(packages, specifier) {
  const split = splitPackageSpecifier(specifier);
  if (split.kind !== "package") {
    return { ok: false, code: "not-package", specifier };
  }
  const pkg = packages.get(split.name);
  if (!pkg) return { ok: false, code: "unknown-package", specifier };
  if (pkg.private) return { ok: false, code: "private-package", specifier, package: pkg.name };
  let target = null;
  if (pkg.exports) target = exportEntry(pkg.exports, split.subpath);
  else if (!split.subpath) target = pkg.main ?? "index.js";
  if (target == null) {
    return {
      ok: false,
      code: "missing-export",
      specifier,
      package: pkg.name,
      version: pkg.version,
    };
  }
  return {
    ok: true,
    specifier,
    package: pkg.name,
    version: pkg.version,
    subpath: split.subpath,
    target,
    public: true,
  };
}

export function shippedBins(packages) {
  const bins = new Map();
  for (const pkg of [...packages.values()].sort((a, b) => a.name.localeCompare(b.name))) {
    if (pkg.private || pkg.bin == null) continue;
    if (typeof pkg.bin === "string") {
      const name = pkg.name.includes("/")
        ? pkg.name.slice(pkg.name.lastIndexOf("/") + 1)
        : pkg.name;
      bins.set(name, { package: pkg.name, path: pkg.bin, version: pkg.version });
      continue;
    }
    for (const [name, binPath] of Object.entries(pkg.bin).sort(([a], [b]) => a.localeCompare(b))) {
      bins.set(name, { package: pkg.name, path: binPath, version: pkg.version });
    }
  }
  return bins;
}

export function resolveShippedCommand(bins, command) {
  const name = command.trim().split(/\s+/)[0];
  if (!name) return { ok: false, code: "empty-command", command };
  if (name === "cargo" || name === "rustc" || name === "xtask") {
    return { ok: false, code: "unshipped-toolchain", command, name };
  }
  const bin = bins.get(name);
  if (!bin) return { ok: false, code: "unshipped-command", command, name };
  return { ok: true, command, name, ...bin };
}

function defaultRead(absPath) {
  return readFileSync(absPath, "utf8");
}

function readAbs(absPath, overlays, repoRoot) {
  const rel = posixRel(repoRoot, absPath);
  if (overlays && Object.prototype.hasOwnProperty.call(overlays, rel)) {
    const value = overlays[rel];
    if (value == null) {
      const err = new Error(`ENOENT: ${rel}`);
      err.code = "ENOENT";
      throw err;
    }
    return value;
  }
  return defaultRead(absPath);
}

function existsAbs(absPath, overlays, repoRoot) {
  const rel = posixRel(repoRoot, absPath);
  if (overlays && Object.prototype.hasOwnProperty.call(overlays, rel)) return overlays[rel] != null;
  return existsSync(absPath);
}

export function extractSpecifiers(source) {
  const found = [];
  SPECIFIER_RE.lastIndex = 0;
  let match;
  while ((match = SPECIFIER_RE.exec(source))) found.push(match[1]);
  return found;
}

export function extractMarkdownHrefs(source) {
  const found = [];
  MD_LINK_RE.lastIndex = 0;
  let match;
  while ((match = MD_LINK_RE.exec(source))) found.push(match[1].trim());
  return found;
}

function isInternalRelative(repoRoot, fromDir, specifier) {
  if (!specifier.startsWith(".")) return false;
  const resolved = resolve(fromDir, specifier);
  const rel = posixRel(repoRoot, resolved);
  if (rel.startsWith("..")) return true;
  return INTERNAL_DIR_RE.test(rel);
}

function lintRegisterCount(rulesSource) {
  const fn = rulesSource.split("fn register_builtin_rules")[1];
  if (!fn) return 0;
  const body = fn.split("\n#[cfg(test)]")[0];
  return [...body.matchAll(/registry\.register\(/g)].length;
}

function sourceDigest(entries) {
  const lines = entries.map(({ path, bytes }) => `${path}:${sha256Text(bytes)}`).sort();
  return sha256Text(lines.join("\n") + "\n");
}

function err(code, message, extra = {}) {
  return { code, message, ...extra };
}

async function runTypeinfoCheck(repoRoot, signal) {
  const script = join(repoRoot, "scripts", "gen-typeinfo-ignore-manifest.mjs");
  return await new Promise((resolvePromise) => {
    const child = spawn(process.execPath, [script, "--check"], {
      cwd: repoRoot,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    const onAbort = () => {
      child.kill();
    };
    if (signal) {
      if (signal.aborted) onAbort();
      else signal.addEventListener("abort", onAbort, { once: true });
    }
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("close", (code, killedBy) => {
      if (signal) signal.removeEventListener("abort", onAbort);
      resolvePromise({ code: code ?? 1, signal: killedBy ?? null, stdout, stderr });
    });
  });
}

export async function validate(options = {}) {
  const repoRoot = resolve(options.repoRoot ?? DEFAULT_REPO_ROOT);
  const examplesRoot = resolve(options.examplesRoot ?? join(repoRoot, "examples", "reference"));
  const overlays = options.overlays ?? {};
  const signal = options.signal;
  const errors = [];
  const read = (abs) => readAbs(abs, overlays, repoRoot);
  const exists = (abs) => existsAbs(abs, overlays, repoRoot);

  const receipt = {
    completenessState: "complete",
    harness: HARNESS_ID,
    sourceRevisions: { digest: null },
    projectConfiguration: {
      examplesRoot: posixRel(repoRoot, examplesRoot),
      manifest: "examples/reference/manifest.json",
    },
    engineIdentity: { node: process.version, harness: HARNESS_ID },
    hostIdentity: { platform: process.platform, arch: process.arch },
    examples: [],
    generatedReference: null,
    links: [],
    capabilities: [],
    errors: [],
  };

  const fail = (item) => {
    errors.push(item);
  };

  if (signal?.aborted) {
    receipt.completenessState = "cancelled";
    receipt.errors = [err("cancelled", "validation aborted before work")];
    return receipt;
  }

  const planPath = join(
    repoRoot,
    "tests",
    "documentation",
    "DOC0",
    "products",
    "generated-reference-plan.v1.json",
  );
  if (!exists(planPath)) {
    fail(
      err("missing-reference-plan", "generated-reference plan is required", {
        path: posixRel(repoRoot, planPath),
      }),
    );
    receipt.completenessState = "failed";
    receipt.errors = errors;
    return receipt;
  }
  const plan = JSON.parse(read(planPath));
  const catalogSurfaces = new Set(plan.capabilityMatrix?.surfaceIds ?? []);
  const expectedLint = plan.lintReference?.ruleCount ?? null;

  const packages = loadWorkspacePackages(repoRoot, (abs) => read(abs));
  const bins = shippedBins(packages);

  const digestEntries = [];
  const manifestPath = join(examplesRoot, "manifest.json");
  if (!exists(manifestPath)) {
    fail(
      err("missing-manifest", "public example home has no manifest", {
        path: posixRel(repoRoot, manifestPath),
      }),
    );
    receipt.completenessState = "failed";
    receipt.errors = errors;
    return receipt;
  }
  const manifestBytes = read(manifestPath);
  digestEntries.push({ path: "examples/reference/manifest.json", bytes: manifestBytes });
  const manifest = JSON.parse(manifestBytes);
  if (!Array.isArray(manifest.examples) || manifest.examples.length === 0) {
    fail(err("empty-examples", "public example home declares no examples"));
  }

  const byId = new Map();
  for (const example of manifest.examples ?? []) {
    if (!example?.id) {
      fail(err("partial-example", "example is missing id"));
      continue;
    }
    if (byId.has(example.id))
      fail(err("duplicate-example", "duplicate example id", { id: example.id }));
    byId.set(example.id, example);
  }

  const discovered = options.discoveryOrder ? [...options.discoveryOrder] : [...byId.keys()];
  for (const id of byId.keys()) {
    if (!discovered.includes(id)) discovered.push(id);
  }

  const exampleRows = [];
  for (const id of discovered) {
    if (signal?.aborted) {
      receipt.completenessState = "cancelled";
      receipt.errors = [...errors, err("cancelled", "validation aborted during example discovery")];
      receipt.examples = exampleRows.sort((a, b) => a.id.localeCompare(b.id));
      return receipt;
    }
    const example = byId.get(id);
    if (!example) {
      fail(
        err("unknown-discovery-id", "discovery named an example absent from the manifest", { id }),
      );
      continue;
    }
    const files = example.files ?? [];
    if (files.length === 0) {
      fail(err("partial-example", "example declares no files", { id }));
      continue;
    }
    const declaredImports = new Set(example.imports ?? []);
    const declaredCommands = [...(example.commands ?? [])];
    const seenSpecifiers = [];
    let fileOk = true;
    for (const relFile of files) {
      const abs = join(examplesRoot, relFile);
      if (!exists(abs)) {
        fail(err("missing-source", "example source is missing", { id, file: relFile }));
        fileOk = false;
        continue;
      }
      const bytes = read(abs);
      digestEntries.push({ path: `examples/reference/${relFile.split(sep).join("/")}`, bytes });
      const fromDir = dirname(abs);
      if (relFile.endsWith(".json")) {
        const json = JSON.parse(bytes);
        if (Array.isArray(json.commands)) declaredCommands.push(...json.commands);
      }
      for (const specifier of extractSpecifiers(bytes)) {
        seenSpecifiers.push(specifier);
        if (isInternalRelative(repoRoot, fromDir, specifier) || specifier.includes("/src/")) {
          fail(
            err("internal-import", "example compiles only against source internals", {
              id,
              file: relFile,
              specifier,
            }),
          );
          fileOk = false;
          continue;
        }
        if (specifier.startsWith("file:")) {
          fail(
            err("internal-import", "file: specifiers are not public exports", { id, specifier }),
          );
          fileOk = false;
        }
      }
      if (relFile.endsWith(".md")) {
        for (const href of extractMarkdownHrefs(bytes)) {
          if (/^[a-z]+:/i.test(href) || href.startsWith("#")) continue;
          const target = resolve(fromDir, href.split("#")[0]);
          receipt.links.push({
            from: `examples/reference/${relFile.split(sep).join("/")}`,
            href,
            ok: exists(target),
          });
          if (!exists(target)) {
            fail(err("broken-link", "example link does not resolve", { id, file: relFile, href }));
          }
        }
      }
    }
    for (const specifier of declaredImports) {
      const split = splitPackageSpecifier(specifier);
      if (split.kind === "relative") continue;
      if ((example.peers ?? []).includes(specifier) || (example.peers ?? []).includes(split.name)) {
        continue;
      }
      const resolved = resolvePackageExport(packages, specifier);
      if (!resolved.ok) {
        fail(
          err("unpublished-export", "example import is not a shipped public export", {
            id,
            specifier,
            reason: resolved.code,
          }),
        );
        fileOk = false;
      }
    }
    for (const specifier of seenSpecifiers) {
      const split = splitPackageSpecifier(specifier);
      if (split.kind !== "package") continue;
      if ((example.peers ?? []).includes(specifier) || (example.peers ?? []).includes(split.name)) {
        continue;
      }
      if (!packages.has(split.name)) continue;
      if (!declaredImports.has(specifier) && !declaredImports.has(split.name)) {
        const resolved = resolvePackageExport(packages, specifier);
        if (!resolved.ok) {
          fail(
            err("unpublished-export", "source import is not a shipped public export", {
              id,
              specifier,
              reason: resolved.code,
            }),
          );
          fileOk = false;
        }
      }
    }
    const uniqueCommands = [...new Set(declaredCommands)];
    for (const command of uniqueCommands) {
      const resolved = resolveShippedCommand(bins, command);
      if (!resolved.ok) {
        fail(
          err("unshipped-command", "example command is absent from shipped package bins", {
            id,
            command,
            reason: resolved.code,
          }),
        );
        fileOk = false;
      }
    }
    const surfaces = example.surfaces ?? [];
    for (const surface of surfaces) {
      const known = catalogSurfaces.has(surface);
      receipt.capabilities.push({ id, surface, ok: known });
      if (!known) {
        fail(
          err("unknown-surface", "example cites a surface absent from the capability catalog", {
            id,
            surface,
          }),
        );
        fileOk = false;
      }
    }
    exampleRows.push({
      id,
      ok: fileOk && errors.every((item) => item.id !== id),
      files,
      imports: [...declaredImports].sort(),
      commands: uniqueCommands.sort(),
      surfaces: [...surfaces].sort(),
      proofClass: example.proofClass ?? "static-proof",
    });
  }

  exampleRows.sort((a, b) => a.id.localeCompare(b.id));
  receipt.examples = exampleRows;

  for (const name of readdirSync(examplesRoot).sort()) {
    if (!name.endsWith(".md")) continue;
    const abs = join(examplesRoot, name);
    if (!exists(abs) || !statSync(abs).isFile()) continue;
    const bytes = read(abs);
    digestEntries.push({ path: `examples/reference/${name}`, bytes });
    for (const href of extractMarkdownHrefs(bytes)) {
      if (/^[a-z]+:/i.test(href) || href.startsWith("#")) continue;
      const target = resolve(examplesRoot, href.split("#")[0]);
      receipt.links.push({
        from: `examples/reference/${name}`,
        href,
        ok: exists(target),
      });
      if (!exists(target))
        fail(err("broken-link", "example home link does not resolve", { file: name, href }));
    }
  }

  receipt.links.sort((a, b) => `${a.from}:${a.href}`.localeCompare(`${b.from}:${b.href}`));
  receipt.capabilities.sort((a, b) => `${a.id}:${a.surface}`.localeCompare(`${b.id}:${b.surface}`));

  const generatedRel =
    plan.generatedPages?.[0]?.path ?? "docs/generated/typeinfo-row-registry-counts.md";
  const generatedAbs = join(repoRoot, ...generatedRel.split("/"));
  const rulesAbs = join(repoRoot, "crates", "verter_diagnostics", "src", "rules", "mod.rs");
  const generated = {
    path: generatedRel,
    exists: exists(generatedAbs),
    typeinfoCheck: null,
    lintRuleCount: null,
  };
  if (!generated.exists) {
    fail(err("stale-generated", "generated reference page is missing", { path: generatedRel }));
  } else {
    const page = read(generatedAbs);
    digestEntries.push({ path: generatedRel, bytes: page });
    if (!page.includes("scripts/gen-typeinfo-ignore-manifest.mjs")) {
      fail(
        err("stale-generated", "generated reference page is not bound to its generator", {
          path: generatedRel,
        }),
      );
    }
  }
  if (exists(rulesAbs)) {
    generated.lintRuleCount = lintRegisterCount(read(rulesAbs));
    if (expectedLint != null && generated.lintRuleCount !== expectedLint) {
      fail(
        err(
          "lint-count-drift",
          "live lint registry count drifted from the generated-reference plan",
          {
            live: generated.lintRuleCount,
            expected: expectedLint,
          },
        ),
      );
    }
  } else {
    fail(err("missing-lint-registry", "lint registry source is missing"));
  }

  if (options.skipTypeinfoCheck !== true) {
    if (signal?.aborted) {
      receipt.completenessState = "cancelled";
      receipt.errors = [
        ...errors,
        err("cancelled", "validation aborted before generated-reference check"),
      ];
      receipt.sourceRevisions.digest = sourceDigest(digestEntries);
      return receipt;
    }
    const check = await runTypeinfoCheck(repoRoot, signal);
    generated.typeinfoCheck = { code: check.code, signal: check.signal };
    if (signal?.aborted || check.signal) {
      receipt.completenessState = "cancelled";
      receipt.generatedReference = generated;
      receipt.errors = [...errors, err("cancelled", "generated-reference check was cancelled")];
      receipt.sourceRevisions.digest = sourceDigest(digestEntries);
      return receipt;
    }
    if (check.code !== 0) {
      fail(
        err("stale-generated", "generated reference failed the existing freshness command", {
          command: "pnpm gen:typeinfo-manifest:check",
          code: check.code,
        }),
      );
    }
  }

  receipt.generatedReference = generated;
  receipt.sourceRevisions.digest = sourceDigest(digestEntries);

  if (
    options.priorReceipt &&
    options.priorReceipt.sourceRevisions?.digest === receipt.sourceRevisions.digest
  ) {
    if (options.priorReceipt.completenessState !== "complete") {
      fail(err("stale-cache", "incremental cache is not a complete receipt"));
    } else if (
      JSON.stringify(canonicalizeReceipt(options.priorReceipt)) !==
      JSON.stringify(canonicalizeReceipt({ ...receipt, errors }))
    ) {
      fail(err("incremental-mismatch", "incremental receipt diverged from the fresh digest match"));
    }
  }

  const missing = errors.some(
    (item) => item.code === "missing-source" || item.code === "partial-example",
  );
  const stale = errors.some(
    (item) => item.code === "stale-generated" || item.code === "stale-cache",
  );
  if (errors.length > 0) {
    receipt.completenessState = stale ? "stale" : missing ? "partial" : "failed";
  }
  receipt.errors = errors;
  receipt.examples = exampleRows.map((row) => ({
    ...row,
    ok: !errors.some((item) => item.id === row.id),
  }));
  return receipt;
}

export function canonicalizeReceipt(receipt) {
  const clone = JSON.parse(JSON.stringify(receipt));
  clone.examples?.sort((a, b) => a.id.localeCompare(b.id));
  clone.links?.sort((a, b) => `${a.from}:${a.href}`.localeCompare(`${b.from}:${b.href}`));
  clone.capabilities?.sort((a, b) => `${a.id}:${a.surface}`.localeCompare(`${b.id}:${b.surface}`));
  clone.errors?.sort((a, b) =>
    `${a.code}:${a.id ?? ""}:${a.specifier ?? a.command ?? a.path ?? ""}`.localeCompare(
      `${b.code}:${b.id ?? ""}:${b.specifier ?? b.command ?? b.path ?? ""}`,
    ),
  );
  return clone;
}

async function main() {
  const skipTypeinfoCheck = process.argv.includes("--skip-typeinfo");
  const receipt = await validate({ repoRoot: DEFAULT_REPO_ROOT, skipTypeinfoCheck });
  const canonical = canonicalizeReceipt(receipt);
  if (receipt.completenessState !== "complete") {
    process.stderr.write(
      `${JSON.stringify({ completenessState: receipt.completenessState, errors: receipt.errors }, null, 2)}\n`,
    );
    process.exitCode = 1;
    return;
  }
  process.stdout.write(
    `${HARNESS_ID}: PASS examples=${canonical.examples.length} digest=${canonical.sourceRevisions.digest} lint=${canonical.generatedReference?.lintRuleCount}\n`,
  );
}

const isMain =
  Boolean(process.argv[1]) && pathToFileURL(resolve(process.argv[1])).href === import.meta.url;

if (isMain) {
  await main();
}
