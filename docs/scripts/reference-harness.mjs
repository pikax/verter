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
import { dirname, extname, join, relative, resolve, sep } from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawn } from "node:child_process";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_REPO_ROOT = resolve(SCRIPT_DIR, "..", "..");

const SPECIFIER_RE = /(?:from\s+|import\s*\(\s*|require\s*\(\s*|import\s+)["']([^"']+)["']/g;
const MD_LINK_RE = /\[[^\]]*]\(([^)]+)\)/g;
const INTERNAL_DIR_RE = /^(packages\/[^/]+\/src\/|crates\/|scripts\/)/;

const REQUIRED_SDK_GUIDE_TOPICS = [
  "compatibility",
  "contribution",
  "debugging",
  "isolation",
  "packaging",
  "permissions",
];
const REQUIRED_RECIPE_TOPICS = [
  "accessibility",
  "compatibility",
  "css",
  "debug",
  "performance",
  "runtime",
  "security",
  "tests",
];
const JOURNEY_EXECUTION_CLASSES = ["NodeOnly", "NativeOnly"];
const JOURNEY_STEP_TIMEOUT_MS = 60_000;
const EXECUTABLE_SOURCE_EXTENSIONS = new Set([".ts", ".mts", ".cts", ".js", ".mjs", ".cjs"]);

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
      bins.set(name, { package: pkg.name, dir: pkg.dir, path: pkg.bin, version: pkg.version });
      continue;
    }
    for (const [name, binPath] of Object.entries(pkg.bin).sort(([a], [b]) => a.localeCompare(b))) {
      bins.set(name, { package: pkg.name, dir: pkg.dir, path: binPath, version: pkg.version });
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

function isFileAbs(absPath, overlays, repoRoot) {
  const rel = posixRel(repoRoot, absPath);
  if (overlays && Object.prototype.hasOwnProperty.call(overlays, rel)) return overlays[rel] != null;
  return existsSync(absPath) && statSync(absPath).isFile();
}

function listNames(absDir, overlays, repoRoot) {
  const names = new Set();
  if (existsSync(absDir) && statSync(absDir).isDirectory()) {
    for (const name of readdirSync(absDir)) names.add(name);
  }
  const relDir = posixRel(repoRoot, absDir);
  const prefix = relDir ? `${relDir}/` : "";
  if (overlays) {
    for (const [rel, value] of Object.entries(overlays)) {
      if (!rel.startsWith(prefix)) continue;
      const rest = rel.slice(prefix.length);
      if (!rest || rest.includes("/")) continue;
      if (value == null) names.delete(rest);
      else names.add(rest);
    }
  }
  return [...names].sort();
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

function runJourneyProcess(repoRoot, args, timeoutMs, signal) {
  return new Promise((resolvePromise) => {
    const child = spawn(process.execPath, args, {
      cwd: repoRoot,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill();
    }, timeoutMs);
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
      clearTimeout(timer);
      if (signal) signal.removeEventListener("abort", onAbort);
      resolvePromise({
        exitCode: timedOut ? null : code ?? 1,
        signal: killedBy ?? null,
        timedOut,
        stdoutDigest: sha256Text(stdout),
        stderr,
      });
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
      manifest: posixRel(repoRoot, join(examplesRoot, "manifest.json")),
    },
    engineIdentity: { node: process.version, harness: HARNESS_ID },
    hostIdentity: { platform: process.platform, arch: process.arch },
    examples: [],
    sdkGuides: null,
    recipes: null,
    journeys: [],
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
  digestEntries.push({ path: posixRel(repoRoot, manifestPath), bytes: manifestBytes });
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
  const exampleFacts = new Map();

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
      receipt.sourceRevisions.digest = sourceDigest(digestEntries);
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
      digestEntries.push({ path: posixRel(repoRoot, abs), bytes });
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
            from: posixRel(repoRoot, abs),
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
      if (split.kind === "relative" || split.kind === "node") continue;
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
    exampleFacts.set(id, { files, commands: uniqueCommands });
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

  for (const name of listNames(examplesRoot, overlays, repoRoot)) {
    if (!name.endsWith(".md")) continue;
    const abs = join(examplesRoot, name);
    if (!isFileAbs(abs, overlays, repoRoot)) continue;
    const bytes = read(abs);
    digestEntries.push({ path: posixRel(repoRoot, abs), bytes });
    for (const href of extractMarkdownHrefs(bytes)) {
      if (/^[a-z]+:/i.test(href) || href.startsWith("#")) continue;
      const target = resolve(examplesRoot, href.split("#")[0]);
      receipt.links.push({
        from: posixRel(repoRoot, abs),
        href,
        ok: exists(target),
      });
      if (!exists(target))
        fail(err("broken-link", "example home link does not resolve", { file: name, href }));
    }
  }

  receipt.links.sort((a, b) => `${a.from}:${a.href}`.localeCompare(`${b.from}:${b.href}`));
  receipt.capabilities.sort((a, b) => `${a.id}:${a.surface}`.localeCompare(`${b.id}:${b.surface}`));

  const sdkModel = manifest.sdkGuides ?? null;
  if (sdkModel != null) {
    if (signal?.aborted) {
      receipt.completenessState = "cancelled";
      receipt.errors = [...errors, err("cancelled", "validation aborted before sdk guide check")];
      receipt.sourceRevisions.digest = sourceDigest(digestEntries);
      return receipt;
    }
    const sdk = { gateEnforced: options.sdkGate === true, index: null, topics: [] };
    const declaredTopics = Array.isArray(sdkModel.topics) ? sdkModel.topics : [];
    const declaredIds = new Set(
      declaredTopics.map((topic) => topic?.id).filter((id) => typeof id === "string"),
    );
    const missingTopics = REQUIRED_SDK_GUIDE_TOPICS.filter((id) => !declaredIds.has(id));
    if (missingTopics.length > 0) {
      fail(
        err("sdk-model-incomplete", "sdk guide model is missing required charter topics", {
          missing: missingTopics,
        }),
      );
    }
    for (const id of declaredIds) {
      if (!REQUIRED_SDK_GUIDE_TOPICS.includes(id)) {
        fail(
          err("sdk-model-unknown-topic", "sdk guide topic is not part of the charter model", {
            id,
          }),
        );
      }
    }

    let listedPages = new Set();
    const indexRelPath = sdkModel.index;
    if (typeof indexRelPath !== "string" || !indexRelPath.endsWith(".md")) {
      fail(err("sdk-index-missing", "sdk guide model declares no markdown index page"));
    } else {
      sdk.index = indexRelPath;
      const indexAbs = join(examplesRoot, indexRelPath);
      if (!exists(indexAbs)) {
        fail(
          err("sdk-page-missing", "sdk guide index page is missing", {
            id: "index",
            page: indexRelPath,
          }),
        );
      } else {
        const bytes = read(indexAbs);
        digestEntries.push({ path: posixRel(repoRoot, indexAbs), bytes });
        listedPages = new Set(
          extractMarkdownHrefs(bytes)
            .filter((href) => !/^[a-z]+:/i.test(href) && !href.startsWith("#"))
            .map((href) => posixRel(examplesRoot, resolve(dirname(indexAbs), href.split("#")[0]))),
        );
      }
    }

    const orderedTopics = [...declaredTopics].sort((a, b) =>
      String(a?.id ?? "").localeCompare(String(b?.id ?? "")),
    );
    for (const topic of orderedTopics) {
      const id = typeof topic?.id === "string" ? topic.id : null;
      if (!id) continue;
      const pageRelPath = typeof topic.page === "string" ? topic.page : null;
      let topicOk = true;
      if (pageRelPath == null || !pageRelPath.endsWith(".md")) {
        fail(
          err("sdk-page-missing", "sdk guide topic declares no markdown page", { id, page: null }),
        );
        topicOk = false;
      } else {
        const pageAbs = join(examplesRoot, pageRelPath);
        if (!exists(pageAbs)) {
          fail(
            err("sdk-page-missing", "sdk guide topic page is missing", { id, page: pageRelPath }),
          );
          topicOk = false;
        } else {
          if (!listedPages.has(pageRelPath)) {
            fail(
              err("sdk-page-unlisted", "sdk guide topic page is not listed by the index", {
                id,
                page: pageRelPath,
              }),
            );
            topicOk = false;
          }
          const bytes = read(pageAbs);
          digestEntries.push({ path: posixRel(repoRoot, pageAbs), bytes });
          for (const href of extractMarkdownHrefs(bytes)) {
            if (/^[a-z]+:/i.test(href) || href.startsWith("#")) continue;
            const target = resolve(dirname(pageAbs), href.split("#")[0]);
            receipt.links.push({ from: posixRel(repoRoot, pageAbs), href, ok: exists(target) });
            if (!exists(target)) {
              fail(
                err("broken-link", "sdk guide page link does not resolve", {
                  id,
                  file: pageRelPath,
                  href,
                }),
              );
              topicOk = false;
            }
          }
        }
      }
      const status = topic.status;
      if (status !== "pending" && status !== "supplied") {
        fail(
          err("sdk-slot-status-invalid", "sdk guide slot status must be pending or supplied", {
            id,
            status: status ?? null,
          }),
        );
        topicOk = false;
      }
      if (status === "pending") {
        if (typeof topic.producingNode !== "string" || topic.producingNode.length === 0) {
          fail(err("sdk-slot-unowned", "pending sdk guide slot names no producing node", { id }));
          topicOk = false;
        }
        if (topic.exampleId != null) {
          fail(
            err("sdk-pending-slot-bound", "pending sdk guide slot must not present an example", {
              id,
              exampleId: topic.exampleId,
            }),
          );
          topicOk = false;
        }
      } else if (status === "supplied") {
        const example = topic.exampleId != null ? byId.get(topic.exampleId) : undefined;
        if (example == null) {
          fail(
            err("sdk-slot-unbound", "supplied sdk guide slot binds no manifest example", {
              id,
              exampleId: topic.exampleId ?? null,
            }),
          );
          topicOk = false;
        } else {
          const files = example.files ?? [];
          const executable = files.some((file) => EXECUTABLE_SOURCE_EXTENSIONS.has(extname(file)));
          if (!executable) {
            fail(
              err(
                "static-sdk-example",
                "supplied sdk guide example has no file with an executable extension",
                { id, exampleId: topic.exampleId },
              ),
            );
            topicOk = false;
          }
          const hasEntry =
            (example.commands ?? []).length > 0 || (example.imports ?? []).length > 0;
          if (!hasEntry) {
            fail(
              err(
                "sdk-example-without-entry",
                "supplied sdk guide example declares no shipped command or public import",
                { id, exampleId: topic.exampleId },
              ),
            );
            topicOk = false;
          }
        }
      }
      if (options.sdkGate === true && status !== "supplied") {
        fail(
          err("sdk-gate-unsatisfied", "the sdk documentation gate requires every topic supplied", {
            id,
            status: status ?? null,
          }),
        );
        topicOk = false;
      }
      sdk.topics.push({
        id,
        page: pageRelPath,
        status: status ?? null,
        producingNode: topic.producingNode ?? null,
        exampleId: topic.exampleId ?? null,
        ok: topicOk,
      });
    }
    receipt.links.sort((a, b) => `${a.from}:${a.href}`.localeCompare(`${b.from}:${b.href}`));
    receipt.sdkGuides = sdk;
  }

  const recipeModel = manifest.recipes ?? null;
  if (recipeModel != null || options.recipesGate === true) {
    if (signal?.aborted) {
      receipt.completenessState = "cancelled";
      receipt.errors = [...errors, err("cancelled", "validation aborted before recipe check")];
      receipt.sourceRevisions.digest = sourceDigest(digestEntries);
      return receipt;
    }
    const recipes = { gateEnforced: options.recipesGate === true, index: null, topics: [] };
    const declaredTopics = Array.isArray(recipeModel?.topics) ? recipeModel.topics : [];
    const declaredRecipeIds = new Set(
      declaredTopics.map((topic) => topic?.id).filter((id) => typeof id === "string"),
    );
    const missingRecipeTopics = REQUIRED_RECIPE_TOPICS.filter((id) => !declaredRecipeIds.has(id));
    if (missingRecipeTopics.length > 0) {
      fail(
        err("recipes-model-incomplete", "recipe model is missing required charter topics", {
          missing: missingRecipeTopics,
        }),
      );
    }
    for (const id of declaredRecipeIds) {
      if (!REQUIRED_RECIPE_TOPICS.includes(id)) {
        fail(
          err("recipes-model-unknown-topic", "recipe topic is not part of the charter model", {
            id,
          }),
        );
      }
    }

    let listedRecipePages = new Set();
    const recipeIndexRelPath = recipeModel?.index;
    if (typeof recipeIndexRelPath !== "string" || !recipeIndexRelPath.endsWith(".md")) {
      fail(err("recipes-index-missing", "recipe model declares no markdown index page"));
    } else {
      recipes.index = recipeIndexRelPath;
      const recipeIndexAbs = join(examplesRoot, recipeIndexRelPath);
      if (!exists(recipeIndexAbs)) {
        fail(
          err("recipes-page-missing", "recipe index page is missing", {
            id: "index",
            page: recipeIndexRelPath,
          }),
        );
      } else {
        const bytes = read(recipeIndexAbs);
        digestEntries.push({ path: posixRel(repoRoot, recipeIndexAbs), bytes });
        listedRecipePages = new Set(
          extractMarkdownHrefs(bytes)
            .filter((href) => !/^[a-z]+:/i.test(href) && !href.startsWith("#"))
            .map((href) =>
              posixRel(
                examplesRoot,
                resolve(dirname(recipeIndexAbs), href.split("#")[0]),
              ),
            ),
        );
      }
    }

    const orderedRecipeTopics = [...declaredTopics].sort((a, b) =>
      String(a?.id ?? "").localeCompare(String(b?.id ?? "")),
    );
    for (const topic of orderedRecipeTopics) {
      const id = typeof topic?.id === "string" ? topic.id : null;
      if (!id) continue;
      const pageRelPath = typeof topic.page === "string" ? topic.page : null;
      let topicOk = true;
      if (pageRelPath == null || !pageRelPath.endsWith(".md")) {
        fail(
          err("recipes-page-missing", "recipe topic declares no markdown page", { id, page: null }),
        );
        topicOk = false;
      } else {
        const pageAbs = join(examplesRoot, pageRelPath);
        if (!exists(pageAbs)) {
          fail(
            err("recipes-page-missing", "recipe topic page is missing", { id, page: pageRelPath }),
          );
          topicOk = false;
        } else {
          if (!listedRecipePages.has(pageRelPath)) {
            fail(
              err("recipes-page-unlisted", "recipe topic page is not listed by the index", {
                id,
                page: pageRelPath,
              }),
            );
            topicOk = false;
          }
          const bytes = read(pageAbs);
          digestEntries.push({ path: posixRel(repoRoot, pageAbs), bytes });
          for (const href of extractMarkdownHrefs(bytes)) {
            if (/^[a-z]+:/i.test(href) || href.startsWith("#")) continue;
            const target = resolve(dirname(pageAbs), href.split("#")[0]);
            receipt.links.push({ from: posixRel(repoRoot, pageAbs), href, ok: exists(target) });
            if (!exists(target)) {
              fail(
                err("broken-link", "recipe page link does not resolve", {
                  id,
                  file: pageRelPath,
                  href,
                }),
              );
              topicOk = false;
            }
          }
        }
      }
      const status = topic.status;
      if (status !== "pending" && status !== "supplied") {
        fail(
          err("recipes-slot-status-invalid", "recipe slot status must be pending or supplied", {
            id,
            status: status ?? null,
          }),
        );
        topicOk = false;
      }
      if (status === "pending") {
        if (typeof topic.producingNode !== "string" || topic.producingNode.length === 0) {
          fail(err("recipes-slot-unowned", "pending recipe slot names no producing node", { id }));
          topicOk = false;
        }
        if (topic.exampleId != null) {
          fail(
            err("recipes-pending-slot-bound", "pending recipe slot must not present an example", {
              id,
              exampleId: topic.exampleId,
            }),
          );
          topicOk = false;
        }
      } else if (status === "supplied") {
        const example = topic.exampleId != null ? byId.get(topic.exampleId) : undefined;
        if (example == null) {
          fail(
            err("recipes-slot-unbound", "supplied recipe slot binds no manifest example", {
              id,
              exampleId: topic.exampleId ?? null,
            }),
          );
          topicOk = false;
        } else {
          const facts = exampleFacts.get(topic.exampleId);
          const files = facts?.files ?? [];
          const executable = files.some((file) =>
            EXECUTABLE_SOURCE_EXTENSIONS.has(extname(file)),
          );
          if (!executable) {
            fail(
              err(
                "recipes-static-example",
                "supplied recipe example has no file with an executable extension",
                { id, exampleId: topic.exampleId },
              ),
            );
            topicOk = false;
          }
          const hasEntry =
            (facts?.commands ?? []).length > 0 || (example.imports ?? []).length > 0;
          if (!hasEntry) {
            fail(
              err(
                "recipes-example-without-entry",
                "supplied recipe example declares no shipped command or public import",
                { id, exampleId: topic.exampleId },
              ),
            );
            topicOk = false;
          }
          if ((example.surfaces ?? []).length === 0) {
            fail(
              err(
                "recipes-without-capability-link",
                "supplied recipe example cites no capability surface",
                { id, exampleId: topic.exampleId },
              ),
            );
            topicOk = false;
          }
        }
      }
      if (options.recipesGate === true && status !== "supplied") {
        fail(
          err("recipes-gate-unsatisfied", "the recipes gate requires every topic supplied", {
            id,
            status: status ?? null,
          }),
        );
        topicOk = false;
      }
      recipes.topics.push({
        id,
        page: pageRelPath,
        status: status ?? null,
        producingNode: topic.producingNode ?? null,
        exampleId: topic.exampleId ?? null,
        ok: topicOk,
      });
    }
    if (options.recipesGate === true && recipeModel == null) {
      fail(
        err("recipes-model-incomplete", "the recipes gate requires a declared recipe model", {
          missing: REQUIRED_RECIPE_TOPICS,
        }),
      );
    }
    receipt.links.sort((a, b) => `${a.from}:${a.href}`.localeCompare(`${b.from}:${b.href}`));
    receipt.recipes = recipes;
  }

  const declaredJourneys = manifest.journeys;
  if (declaredJourneys != null && (!Array.isArray(declaredJourneys) || declaredJourneys.length === 0)) {
    fail(err("journeys-empty", "the example home declares an empty journeys model"));
  }
  const journeyRows = [];
  const journeyIds = new Set();
  const structuralJourneyError = (id) => errors.some((item) => item.journey === id);
  for (const journey of Array.isArray(declaredJourneys) ? declaredJourneys : []) {
    const id = typeof journey?.id === "string" && journey.id.length > 0 ? journey.id : null;
    if (!id) {
      fail(err("journey-missing-id", "journey declares no id"));
      continue;
    }
    let journeyOk = true;
    if (journeyIds.has(id)) {
      fail(err("duplicate-journey", "duplicate journey id", { journey: id }));
      journeyOk = false;
    }
    journeyIds.add(id);
    if (typeof journey.description !== "string" || journey.description.length === 0) {
      fail(err("journey-description-missing", "journey declares no description", { journey: id }));
      journeyOk = false;
    }
    if (!JOURNEY_EXECUTION_CLASSES.includes(journey.executionClass)) {
      fail(
        err("journey-class-invalid", "journey execution class is unknown", {
          journey: id,
          executionClass: journey.executionClass ?? null,
        }),
      );
      journeyOk = false;
    }
    if (
      journey.pins != null &&
      (typeof journey.pins !== "object" ||
        Array.isArray(journey.pins) ||
        Object.keys(journey.pins).length === 0 ||
        !Object.values(journey.pins).every((value) => typeof value === "string"))
    ) {
      fail(
        err("journey-pins-invalid", "journey pins must map names to version strings", {
          journey: id,
        }),
      );
      journeyOk = false;
    }
    const steps = Array.isArray(journey.steps) ? journey.steps : [];
    if (steps.length === 0) {
      fail(err("journey-without-steps", "journey declares no steps", { journey: id }));
      journeyOk = false;
    }
    const stepRows = [];
    for (const step of steps) {
      const stepRow = {
        exampleId: typeof step?.exampleId === "string" ? step.exampleId : null,
        file: typeof step?.file === "string" ? step.file : null,
        command: typeof step?.command === "string" ? step.command : null,
        args: [],
        expectExit: 0,
      };
      let stepOk = true;
      if (!byId.has(stepRow.exampleId)) {
        fail(
          err("journey-step-unknown-example", "journey step binds no manifest example", {
            journey: id,
            exampleId: stepRow.exampleId,
          }),
        );
        stepOk = false;
      }
      if ((stepRow.file == null) === (stepRow.command == null)) {
        fail(
          err("journey-step-entry-invalid", "journey step needs exactly one file or command entry", {
            journey: id,
          }),
        );
        stepOk = false;
      }
      if (stepRow.file != null) {
        const facts = stepRow.exampleId != null ? exampleFacts.get(stepRow.exampleId) : undefined;
        const files = facts?.files ?? [];
        if (
          !files.includes(stepRow.file) ||
          !EXECUTABLE_SOURCE_EXTENSIONS.has(extname(stepRow.file))
        ) {
          fail(
            err(
              "journey-step-file-outside-example",
              "journey step file is not an executable file of the bound example",
              { journey: id, file: stepRow.file },
            ),
          );
          stepOk = false;
        }
        if (journey.executionClass === "NativeOnly") {
          fail(
            err("journey-class-mismatch", "NativeOnly journeys run shipped commands, not files", {
              journey: id,
            }),
          );
          stepOk = false;
        }
      }
      if (stepRow.command != null) {
        const resolved = resolveShippedCommand(bins, stepRow.command);
        if (!resolved.ok) {
          fail(
            err("journey-step-unshipped-command", "journey command is not a shipped bin", {
              journey: id,
              command: stepRow.command,
              reason: resolved.code,
            }),
          );
          stepOk = false;
        }
        const facts = stepRow.exampleId != null ? exampleFacts.get(stepRow.exampleId) : undefined;
        if (!(facts?.commands ?? []).includes(resolved.name)) {
          fail(
            err(
              "journey-step-command-outside-example",
              "journey command is not declared by the bound example",
              { journey: id, command: stepRow.command, exampleId: stepRow.exampleId },
            ),
          );
          stepOk = false;
        }
        if (journey.executionClass === "NodeOnly") {
          fail(
            err("journey-class-mismatch", "NodeOnly journeys run example files, not commands", {
              journey: id,
            }),
          );
          stepOk = false;
        }
      }
      if (
        step?.args != null &&
        (!Array.isArray(step.args) || !step.args.every((arg) => typeof arg === "string"))
      ) {
        fail(
          err("journey-step-args-invalid", "journey step args must be strings", { journey: id }),
        );
        stepOk = false;
      } else if (Array.isArray(step?.args)) {
        stepRow.args = [...step.args];
      }
      if (step?.expectExit != null && !Number.isInteger(step.expectExit)) {
        fail(
          err("journey-step-expect-invalid", "journey step expectExit must be an integer", {
            journey: id,
          }),
        );
        stepOk = false;
      } else if (Number.isInteger(step?.expectExit)) {
        stepRow.expectExit = step.expectExit;
      }
      stepRows.push(stepRow);
      if (!stepOk) journeyOk = false;
    }
    journeyRows.push({
      id,
      description: journey.description ?? null,
      executionClass: journey.executionClass ?? null,
      pins: journey.pins ?? null,
      steps: stepRows,
      execution: { state: "not-run" },
      ok: journeyOk,
    });
  }
  journeyRows.sort((a, b) => a.id.localeCompare(b.id));

  if (options.runJourneys === true) {
    const timeoutMs = options.journeyStepTimeoutMs ?? JOURNEY_STEP_TIMEOUT_MS;
    for (const row of journeyRows) {
      if (signal?.aborted) break;
      if (structuralJourneyError(row.id)) continue;
      const executedSteps = [];
      let cancelled = false;
      for (let index = 0; index < row.steps.length; index += 1) {
        if (signal?.aborted) {
          cancelled = true;
          break;
        }
        const step = row.steps[index];
        let entry;
        let spawnArgs;
        if (step.file != null) {
          entry = { file: step.file };
          spawnArgs = [join(examplesRoot, step.file), ...step.args];
        } else {
          const bin = bins.get(resolveShippedCommand(bins, step.command).name);
          entry = {
            command: step.command,
            package: bin?.package ?? null,
            packageVersion: bin?.version ?? null,
          };
          spawnArgs = [join(bin.dir, bin.path), ...step.args];
        }
        const outcome = await runJourneyProcess(repoRoot, spawnArgs, timeoutMs, signal);
        if (outcome.timedOut) {
          fail(
            err("journey-step-timeout", "journey step exceeded its bounded runtime", {
              journey: row.id,
              ...entry,
              timeoutMs,
            }),
          );
          executedSteps.push({
            ...entry,
            args: step.args,
            expectExit: step.expectExit,
            exitCode: null,
            timedOut: true,
            stdoutDigest: outcome.stdoutDigest,
            ok: false,
          });
          continue;
        }
        if (signal?.aborted || outcome.signal) {
          cancelled = true;
          executedSteps.push({
            ...entry,
            args: step.args,
            expectExit: step.expectExit,
            exitCode: null,
            stdoutDigest: outcome.stdoutDigest,
            ok: false,
          });
          break;
        }
        const ok = outcome.exitCode === step.expectExit;
        if (!ok) {
          fail(
            err("journey-step-mismatch", "journey step missed its expected exit code", {
              journey: row.id,
              ...entry,
              exitCode: outcome.exitCode,
              expectExit: step.expectExit,
              stderr: outcome.stderr.slice(0, 400),
            }),
          );
        }
        executedSteps.push({
          ...entry,
          args: step.args,
          expectExit: step.expectExit,
          exitCode: outcome.exitCode,
          stdoutDigest: outcome.stdoutDigest,
          ok,
        });
      }
      if (cancelled) {
        row.execution = { state: "cancelled" };
        row.ok = false;
      } else {
        row.execution = { state: "executed", steps: executedSteps };
        row.ok = row.ok && executedSteps.every((step) => step.ok);
      }
    }
    if (signal?.aborted) {
      receipt.completenessState = "cancelled";
      receipt.journeys = journeyRows;
      receipt.errors = [...errors, err("cancelled", "validation aborted during journey execution")];
      receipt.sourceRevisions.digest = sourceDigest(digestEntries);
      return receipt;
    }
  }
  receipt.journeys = journeyRows;

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
    (item) =>
      item.code === "missing-source" ||
      item.code === "partial-example" ||
      item.code === "sdk-page-missing" ||
      item.code === "recipes-page-missing",
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
  clone.sdkGuides?.topics?.sort((a, b) => a.id.localeCompare(b.id));
  clone.recipes?.topics?.sort((a, b) => a.id.localeCompare(b.id));
  clone.journeys?.sort((a, b) => a.id.localeCompare(b.id));
  clone.links?.sort((a, b) => `${a.from}:${a.href}`.localeCompare(`${b.from}:${b.href}`));
  clone.capabilities?.sort((a, b) => `${a.id}:${b.surface}`.localeCompare(`${a.id}:${b.surface}`));
  clone.errors?.sort((a, b) =>
    `${a.code}:${a.id ?? ""}:${a.specifier ?? a.command ?? a.path ?? ""}`.localeCompare(
      `${b.code}:${b.id ?? ""}:${b.specifier ?? b.command ?? b.path ?? ""}`,
    ),
  );
  return clone;
}

async function main() {
  const skipTypeinfoCheck = process.argv.includes("--skip-typeinfo");
  const sdkGate = process.argv.includes("--sdk-gate");
  const recipesGate = process.argv.includes("--recipes-gate");
  const runJourneys = process.argv.includes("--run-journeys");
  const receipt = await validate({
    repoRoot: DEFAULT_REPO_ROOT,
    skipTypeinfoCheck,
    sdkGate,
    recipesGate,
    runJourneys,
  });
  const canonical = canonicalizeReceipt(receipt);
  if (receipt.completenessState !== "complete") {
    process.stderr.write(
      `${JSON.stringify({ completenessState: receipt.completenessState, errors: receipt.errors }, null, 2)}\n`,
    );
    process.exitCode = 1;
    return;
  }
  process.stdout.write(
    `${HARNESS_ID}: PASS examples=${canonical.examples.length} sdk=${canonical.sdkGuides?.topics?.length ?? 0} recipes=${canonical.recipes?.topics?.length ?? 0} journeys=${canonical.journeys?.length ?? 0} digest=${canonical.sourceRevisions.digest} lint=${canonical.generatedReference?.lintRuleCount}\n`,
  );
}

const isMain =
  Boolean(process.argv[1]) && pathToFileURL(resolve(process.argv[1])).href === import.meta.url;

if (isMain) {
  await main();
}
