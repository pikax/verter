#!/usr/bin/env node
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { gunzipSync } from "node:zlib";

import { GitHubAdapterError } from "./errors.mjs";
import { computePublishSet, scanWorkspacePackages } from "../lib/publish-set.mjs";
import { BINARY_FAMILIES, invokedAsEntrypoint, parsePlatformDir } from "../lib/release-publish.mjs";
import { fixExecutableBits, packTarball } from "../release-publish.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../..");
const CLEAN_ROOM_SCRIPT = "scripts/githubctl/clean-room.mjs";
const RUNTIME_CONDITIONS = new Set([
  "import",
  "require",
  "default",
  "node",
  "node-addons",
  "module-sync",
]);
const JS_FILE = /\.(?:m?js|cjs|node)$/u;
// Only declaration files are types-only. A runtime `.ts` target is exercised
// through its declared conditions like any other runtime file (Node refuses to
// strip types under node_modules, so such a target fails as unloadable).
const TYPES_FILE = /\.d\.[cm]?ts$/u;
const TAR_BLOCK = 512;
const MUSL_LINKER = {
  x64: "/lib/ld-musl-x86_64.so.1",
  arm64: "/lib/ld-musl-aarch64.so.1",
};

export const CLEAN_ROOM_KIND = "CleanRoomPublishedArtifact";

function fail(reason, message, extra = {}) {
  return { ok: false, skipped: false, kind: CLEAN_ROOM_KIND, reason, message, ...extra };
}

function inside(inner, outer) {
  const resolvedInner = fs.existsSync(inner) ? fs.realpathSync(inner) : path.resolve(inner);
  const resolvedOuter = fs.existsSync(outer) ? fs.realpathSync(outer) : path.resolve(outer);
  const prefix = resolvedOuter.endsWith(path.sep) ? resolvedOuter : `${resolvedOuter}${path.sep}`;
  return resolvedInner === resolvedOuter || resolvedInner.startsWith(prefix);
}

function npmInvocation(args) {
  const cli = [
    path.join(path.dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js"),
    path.join(
      path.dirname(process.execPath),
      "..",
      "lib",
      "node_modules",
      "npm",
      "bin",
      "npm-cli.js",
    ),
  ].find((candidate) => fs.existsSync(candidate));
  if (cli) return { command: process.execPath, args: [cli, ...args] };
  if (process.platform === "win32") return { command: "npm.cmd", args };
  return { command: "npm", args };
}

function runNpm(args, options = {}) {
  const invocation = npmInvocation(args);
  return spawnSync(invocation.command, invocation.args, {
    encoding: "utf8",
    shell: false,
    ...options,
  });
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function sha256(filePath) {
  return createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

function walkTar(tgzPath, onEntry) {
  const tar = gunzipSync(fs.readFileSync(tgzPath));
  let offset = 0;
  let longName = null;
  while (offset + TAR_BLOCK <= tar.length) {
    const header = tar.subarray(offset, offset + TAR_BLOCK);
    if (header.every((byte) => byte === 0)) break;
    const size = parseInt(
      header.subarray(124, 136).toString("utf8").replace(/\0/g, "").trim() || "0",
      8,
    );
    const typeflag = String.fromCharCode(header[156] || 0x30);
    const data = tar.subarray(offset + TAR_BLOCK, offset + TAR_BLOCK + size);
    const raw = header.subarray(0, 100).toString("utf8").replace(/\0/g, "");
    if (typeflag === "L") longName = data.toString("utf8").replace(/\0/g, "");
    else if (typeflag === "x") {
      const text = data.toString("utf8");
      const match = text.match(/\spath=([^\n]+)/u);
      if (match) longName = match[1];
    } else {
      const name = longName ?? raw;
      longName = null;
      const relative = name.startsWith("package/") ? name.slice("package/".length) : name;
      if (relative && typeflag !== "5") onEntry(relative.replace(/\\/g, "/"), data);
    }
    offset += TAR_BLOCK + Math.ceil(size / TAR_BLOCK) * TAR_BLOCK;
  }
}

function tarNames(tgzPath) {
  const names = [];
  walkTar(tgzPath, (name) => names.push(name));
  return names;
}

function tarFile(tgzPath, relativePath) {
  let found = null;
  walkTar(tgzPath, (name, data) => {
    if (name === relativePath) found = data;
  });
  return found;
}

function packedManifest(tgzPath) {
  const buf = tarFile(tgzPath, "package.json");
  if (!buf) return null;
  return JSON.parse(buf.toString("utf8"));
}

function exportMap(exportsField) {
  if (exportsField == null) return null;
  if (typeof exportsField === "string") return { ".": exportsField };
  if (typeof exportsField !== "object" || Array.isArray(exportsField)) return null;
  const keys = Object.keys(exportsField);
  if (keys.some((key) => key.startsWith("."))) return exportsField;
  return { ".": exportsField };
}

function flattenRuntime(target, inherited = []) {
  if (typeof target === "string")
    return [{ file: target, conditions: inherited.length > 0 ? inherited : ["default"] }];
  if (!target || typeof target !== "object" || Array.isArray(target)) return [];
  const rows = [];
  for (const [key, value] of Object.entries(target)) {
    if (key === "types") continue;
    if (RUNTIME_CONDITIONS.has(key)) rows.push(...flattenRuntime(value, [...inherited, key]));
  }
  return rows;
}

function normalizeFile(file) {
  return String(file).replace(/^\.\//u, "").replace(/\\/g, "/");
}

function specifierFor(name, subpath) {
  if (subpath === "." || subpath === "") return name;
  return `${name}/${subpath.replace(/^\.\//u, "")}`;
}

export function declaredEntrypoints(pkg) {
  const rows = [];
  const seen = new Set();
  const add = (subpath, file, conditions, typesOnly = false) => {
    const normalized = file ? normalizeFile(file) : "";
    const types = typesOnly || TYPES_FILE.test(normalized);
    const conds = types ? [] : conditions.slice();
    const key = `${subpath}\0${conds.slice().sort().join(",")}\0${normalized}\0${types ? "t" : "r"}`;
    if (seen.has(key)) return;
    seen.add(key);
    rows.push({
      subpath,
      file: normalized,
      conditions: conds,
      typesOnly: types,
    });
  };
  const mapped = exportMap(pkg.exports);
  if (mapped) {
    for (const [subpath, target] of Object.entries(mapped)) {
      if (subpath.includes("*")) {
        throw new GitHubAdapterError(
          `wildcard exports are not a clean-room entrypoint: ${pkg.name} ${subpath}`,
        );
      }
      const runtime = flattenRuntime(target);
      if (runtime.length === 0) {
        const typesFile =
          typeof target === "object" && target && typeof target.types === "string"
            ? target.types
            : typeof target === "object" && target?.types && typeof target.types.import === "string"
              ? target.types.import
              : typeof target === "string"
                ? target
                : null;
        add(subpath, typesFile ?? subpath, [], true);
        continue;
      }
      for (const row of runtime) add(subpath, row.file, row.conditions, false);
    }
    return rows;
  }
  if (typeof pkg.module === "string") add(".", pkg.module, ["import"]);
  if (typeof pkg.main === "string") {
    add(".", pkg.main, pkg.type === "module" ? ["import"] : ["require"]);
  }
  const bin = pkg.bin;
  if (typeof bin === "string") add(".", bin, ["bin"]);
  else if (bin && typeof bin === "object") {
    for (const file of Object.values(bin)) add(".", file, ["bin"]);
  }
  return rows;
}

function classifyLoadError(stderr, stdout) {
  const text = `${stderr ?? ""}\n${stdout ?? ""}`;
  if (/Did you mean ['"].*\.js['"]/u.test(text)) return "missing-js-extension";
  const missing = text.match(/Cannot find module ['"]([^'"]+)['"]/u);
  if (/ERR_MODULE_NOT_FOUND/u.test(text) && missing) {
    const spec = missing[1].replace(/\\/g, "/");
    if (!/\.(?:m?js|cjs|json|node)$/u.test(spec)) return "missing-js-extension";
  }
  return "unloadable";
}

function stripYamlComment(line) {
  let quote = null;
  for (let i = 0; i < line.length; i += 1) {
    const ch = line[i];
    if (quote === "'") {
      if (ch === "'") quote = null;
      continue;
    }
    if (quote === '"') {
      if (ch === "\\" && i + 1 < line.length) {
        i += 1;
        continue;
      }
      if (ch === '"') quote = null;
      continue;
    }
    if (ch === "'" || ch === '"') {
      quote = ch;
      continue;
    }
    if (ch === "#" && quote === null) return line.slice(0, i);
  }
  return line;
}

function splitWorkflowJobs(text) {
  const jobs = {};
  const lines = String(text).split(/\r?\n/u);
  let current = null;
  let buffer = [];
  const flush = () => {
    if (current) jobs[current] = buffer.join("\n");
    current = null;
    buffer = [];
  };
  let inJobs = false;
  for (const line of lines) {
    if (/^jobs:\s*$/u.test(line)) {
      inJobs = true;
      continue;
    }
    if (!inJobs) continue;
    const job = line.match(/^  ([A-Za-z0-9_-]+):\s*$/u);
    if (job) {
      flush();
      current = job[1];
      continue;
    }
    if (/^[A-Za-z]/u.test(line)) {
      flush();
      inJobs = false;
      continue;
    }
    if (current) buffer.push(line);
  }
  flush();
  return jobs;
}

function jobLevelIf(body) {
  for (const line of body.split(/\r?\n/u)) {
    if (/^    steps:/u.test(line)) return null;
    const stripped = stripYamlComment(line);
    const match = stripped.match(/^    if:\s*(.+?)\s*$/u);
    if (match) return match[1];
  }
  return null;
}

function stepBlocks(body) {
  const lines = body.split(/\r?\n/u);
  const steps = [];
  let current = null;
  for (const line of lines) {
    if (/^      - /u.test(line)) {
      if (current) steps.push(current);
      current = [line];
      continue;
    }
    if (current) current.push(line);
  }
  if (current) steps.push(current);
  return steps.map((block) => block.join("\n"));
}

function stepRunsCleanRoom(block) {
  for (const line of block.split(/\r?\n/u)) {
    const stripped = stripYamlComment(line);
    if (/\bnode\b/u.test(stripped) && stripped.includes(CLEAN_ROOM_SCRIPT)) return true;
  }
  return false;
}

function stepHasIf(block) {
  return block.split(/\r?\n/u).some((line) => /^\s+if:/u.test(stripYamlComment(line)));
}

export function assertCleanRoomHosted(releaseYml) {
  const jobs = splitWorkflowJobs(releaseYml);
  const hosts = [];
  for (const [name, body] of Object.entries(jobs)) {
    const steps = stepBlocks(body).filter((block) => stepRunsCleanRoom(block));
    if (steps.length > 0) hosts.push({ name, body, steps });
  }
  if (hosts.length === 0) {
    throw new GitHubAdapterError("release.yml must run the clean-room published-artifact check");
  }
  for (const { name, body, steps } of hosts) {
    if (jobLevelIf(body)) {
      throw new GitHubAdapterError(`clean-room check in job ${name} is skipped during rehearsal`);
    }
    if (/continue-on-error:\s*true/u.test(body)) {
      throw new GitHubAdapterError(`clean-room check in job ${name} is skipped during rehearsal`);
    }
    for (const step of steps) {
      if (stepHasIf(step)) {
        throw new GitHubAdapterError(`clean-room check in job ${name} is skipped during rehearsal`);
      }
    }
  }
  return { kind: CLEAN_ROOM_KIND, hosted: true, skipped: false };
}

function claimWorkDir(workDir) {
  if (fs.existsSync(workDir)) {
    if (fs.readdirSync(workDir).length > 0) {
      return fail("reused-cache", `clean-room work directory is not empty: ${workDir}`);
    }
  } else {
    fs.mkdirSync(workDir, { recursive: true });
  }
  return null;
}

export function hostLibc() {
  if (process.platform !== "linux") return null;
  try {
    if (process.report) process.report.excludeNetwork = true;
    const header = process.report?.getReport()?.header;
    if (header?.glibcVersionRuntime) return "glibc";
  } catch {
    // fall through to filesystem signals
  }
  if (fs.existsSync("/etc/alpine-release")) return "musl";
  const linker = MUSL_LINKER[process.arch];
  if (linker && fs.existsSync(linker)) return "musl";
  return "glibc";
}

export function platformMatches(pkg) {
  if (Array.isArray(pkg.os) && pkg.os.length > 0 && !pkg.os.includes(process.platform))
    return false;
  if (Array.isArray(pkg.cpu) && pkg.cpu.length > 0 && !pkg.cpu.includes(process.arch)) return false;
  if (Array.isArray(pkg.libc) && pkg.libc.length > 0) {
    const libc = hostLibc();
    if (libc == null || !pkg.libc.includes(libc)) return false;
  }
  return true;
}

function executableFilesFor(unit, repoRoot, pkg) {
  const rel = path.relative(repoRoot, unit.dir).split(path.sep).join("/");
  const parsed = parsePlatformDir(rel);
  if (!parsed) return [];
  const family = BINARY_FAMILIES[parsed.family];
  if (!family?.executable) return [];
  return Array.isArray(pkg.files) ? pkg.files.map(normalizeFile) : [];
}

function packUnit(unit, packDir, repoRoot) {
  const tarball = packTarball({ dir: unit.dir, label: unit.name }, packDir);
  const dest = path.join(packDir, path.basename(tarball));
  if (path.resolve(tarball) !== path.resolve(dest)) fs.copyFileSync(tarball, dest);
  const sourcePkg = fs.existsSync(path.join(unit.dir, "package.json"))
    ? readJson(path.join(unit.dir, "package.json"))
    : { files: [] };
  fixExecutableBits(
    { label: unit.name, executableFiles: executableFilesFor(unit, repoRoot, sourcePkg) },
    dest,
  );
  return dest;
}

function writeSmokeScripts(consumerDir) {
  fs.writeFileSync(
    path.join(consumerDir, "smoke-loader.mjs"),
    `import fs from "node:fs";
import { fileURLToPath } from "node:url";
const logPath = process.env.CLEAN_ROOM_LOAD_LOG;
export async function resolve(specifier, context, nextResolve) {
  const result = await nextResolve(specifier, context);
  if (logPath && typeof result.url === "string" && result.url.startsWith("file:")) {
    try { fs.appendFileSync(logPath, fileURLToPath(result.url) + "\\n"); } catch {}
  }
  return result;
}
`,
  );
  fs.writeFileSync(
    path.join(consumerDir, "smoke-register.mjs"),
    `import { register } from "node:module";
import { pathToFileURL } from "node:url";
register(new URL("./smoke-loader.mjs", import.meta.url));
`,
  );
  fs.writeFileSync(
    path.join(consumerDir, "smoke-import.mjs"),
    `import fs from "node:fs";
const spec = process.argv[2];
const ns = await import(spec);
if (typeof ns.cleanRoomPing === "function") ns.cleanRoomPing();
else if (ns.default == null && Object.keys(ns).length === 0) throw new Error("empty module namespace");
const resolved = import.meta.resolve(spec);
const logPath = process.env.CLEAN_ROOM_LOAD_LOG;
const loaded = logPath && fs.existsSync(logPath)
  ? fs.readFileSync(logPath, "utf8").split(/\\r?\\n/u).filter(Boolean)
  : [];
console.log(JSON.stringify({ ok: true, resolved, loaded }));
`,
  );
  fs.writeFileSync(
    path.join(consumerDir, "smoke-require.cjs"),
    `"use strict";
const fs = require("node:fs");
const spec = process.argv[2];
const mod = require(spec);
if (mod && typeof mod.cleanRoomPing === "function") mod.cleanRoomPing();
else if (mod == null) throw new Error("empty module namespace");
const loaded = Object.keys(require.cache);
console.log(JSON.stringify({ ok: true, resolved: require.resolve(spec), loaded }));
`,
  );
}

function smokeEnv(consumerDir, extra = {}) {
  const env = { ...process.env, NODE_PATH: "", ...extra };
  delete env.NODE_OPTIONS;
  return env;
}

function smoke(consumerDir, spec, condition, logPath) {
  const script = condition === "require" ? "smoke-require.cjs" : "smoke-import.mjs";
  const args =
    condition === "require"
      ? [path.join(consumerDir, script), spec]
      : [
          "--import",
          pathToFileURL(path.join(consumerDir, "smoke-register.mjs")).href,
          path.join(consumerDir, script),
          spec,
        ];
  if (logPath && fs.existsSync(logPath)) fs.rmSync(logPath);
  return spawnSync(process.execPath, args, {
    cwd: consumerDir,
    encoding: "utf8",
    env: smokeEnv(consumerDir, logPath ? { CLEAN_ROOM_LOAD_LOG: logPath } : {}),
    timeout: 30_000,
  });
}

function binInvocation(binPath, flag) {
  if (/\.(?:[cm]?js)$/u.test(binPath)) return [process.execPath, [binPath, flag], {}];
  // A Windows batch entrypoint only runs through the command interpreter.
  if (process.platform === "win32" && /\.(?:cmd|bat)$/iu.test(binPath)) {
    return [
      process.env.ComSpec || "cmd.exe",
      ["/d", "/s", "/c", `"${binPath}" ${flag}`],
      { windowsVerbatimArguments: true },
    ];
  }
  return [binPath, [flag], {}];
}

function smokeBin(binPath, consumerDir) {
  const attempts = [binInvocation(binPath, "--help"), binInvocation(binPath, "--version")];
  let last = null;
  for (const [command, args, extra] of attempts) {
    last = spawnSync(command, args, {
      cwd: consumerDir,
      encoding: "utf8",
      env: smokeEnv(consumerDir),
      timeout: 15_000,
      ...extra,
    });
    if (last.status === 0) return last;
    const text = `${last.stderr ?? ""}\n${last.stdout ?? ""}`;
    if (/ERR_MODULE_NOT_FOUND|Cannot find module|missing-js-extension|Did you mean/u.test(text)) {
      return last;
    }
    if (last.error?.code === "ENOENT") return last;
  }
  // A nonzero exit from both `--help` and `--version` is a failed public
  // call; it is never coerced to success.
  return last;
}

function inspectDeps(pkg, consumerDir, repoRoot, { consumer } = { consumer: false }) {
  if (consumer) {
    if (pkg.patchedDependencies || pkg.pnpm?.patchedDependencies) {
      return fail("consumer-patch", "consumer-side patch cannot satisfy clean-room evidence");
    }
    if (pkg.pnpm?.overrides || pkg.overrides) {
      return fail("workspace-resolved", "lockfile override cannot satisfy clean-room evidence");
    }
  }
  const deps = { ...pkg.dependencies, ...pkg.optionalDependencies };
  for (const [name, spec] of Object.entries(deps)) {
    if (typeof spec === "string" && spec.startsWith("file:")) {
      const target = spec.slice("file:".length);
      const resolved = path.resolve(consumerDir, target);
      if (inside(resolved, repoRoot) && !resolved.endsWith(".tgz")) {
        return fail("source-tree-fallback", `dependency ${name} resolves into the source tree`);
      }
    }
    if (typeof spec === "string" && spec.startsWith("workspace:")) {
      return fail("workspace-resolved", `dependency ${name} uses workspace protocol`);
    }
  }
  return null;
}

export function inspectConsumer(consumerDir, repoRoot) {
  const manifestPath = path.join(consumerDir, "package.json");
  if (!fs.existsSync(manifestPath)) return null;
  const isolated = inspectDeps(readJson(manifestPath), consumerDir, repoRoot, { consumer: true });
  if (isolated) return isolated;
  if (fs.existsSync(path.join(consumerDir, "pnpm-workspace.yaml"))) {
    return fail("workspace-resolved", "consumer is a pnpm workspace");
  }
  return null;
}

function inspectInstalled(consumerDir, repoRoot) {
  const root = path.join(consumerDir, "node_modules");
  if (!fs.existsSync(root)) return null;
  const stack = [root];
  while (stack.length > 0) {
    const dir = stack.pop();
    let entries = [];
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (!entry.isDirectory() || entry.name === ".bin") continue;
      const full = path.join(dir, entry.name);
      if (entry.name.startsWith("@")) {
        stack.push(full);
        continue;
      }
      const pkgPath = path.join(full, "package.json");
      if (fs.existsSync(pkgPath)) {
        const isolated = inspectDeps(readJson(pkgPath), consumerDir, repoRoot, { consumer: false });
        if (isolated) return isolated;
      }
    }
  }
  return null;
}

function publishedUnits(options) {
  if (options.units) return options.units;
  const rootDir = options.repoRoot ?? REPO_ROOT;
  const publishSet = options.publishSet ?? computePublishSet({ rootDir, roots: options.roots });
  const workspace = options.workspace ?? scanWorkspacePackages(path.join(rootDir, "packages"));
  const units = [];
  for (const name of publishSet.npm) {
    const entry = workspace.get(name);
    if (!entry) throw new Error(`publish set names "${name}" but it is not a workspace package`);
    units.push({ name, dir: entry.dir, kind: "package" });
  }
  for (const dir of publishSet.platform ?? []) {
    const abs = path.join(rootDir, dir);
    const pkg = readJson(path.join(abs, "package.json"));
    units.push({ name: pkg.name, dir: abs, kind: "platform" });
  }
  return units;
}

function payloadFiles(pkg, tarFiles) {
  if (Array.isArray(pkg.files) && pkg.files.length > 0) {
    return pkg.files.map(normalizeFile).filter((name) => name && !name.endsWith("/"));
  }
  return tarFiles.filter(
    (name) =>
      name !== "package.json" &&
      !/^readme(?:\.(?:md|txt))?$/iu.test(name) &&
      !/^license(?:\.(?:md|txt))?$/iu.test(name),
  );
}

function isolationFromLoaded(loaded, resolved, consumerDir, repoRoot) {
  const paths = [];
  if (typeof resolved === "string") paths.push(resolved);
  if (Array.isArray(loaded)) {
    for (const item of loaded) {
      if (typeof item !== "string") continue;
      const filePath = item.startsWith("file:") ? fileURLToPath(item) : item;
      paths.push(filePath);
    }
  }
  for (const filePath of paths) {
    if (!filePath) continue;
    if (inside(filePath, consumerDir)) continue;
    if (inside(filePath, repoRoot)) {
      return {
        reason: "source-tree-fallback",
        message: `loaded module resolved into the repository: ${filePath}`,
      };
    }
  }
  if (resolved && !inside(resolved, consumerDir)) {
    return {
      reason: "source-tree-fallback",
      message: `resolved outside the scratch consumer: ${resolved}`,
    };
  }
  return null;
}

function runCleanRoomCheckInner(options = {}) {
  if (options.skip === true) {
    return fail("skipped", "a rehearsal cannot report PASS with the clean-room check skipped", {
      skipped: true,
    });
  }
  const repoRoot = path.resolve(options.repoRoot ?? REPO_ROOT);
  const workDir = path.resolve(
    options.workDir ?? fs.mkdtempSync(path.join(os.tmpdir(), "rel3-clean-room-")),
  );
  const claimed = claimWorkDir(workDir);
  if (claimed) return claimed;

  const packDir = path.join(workDir, "tarballs");
  const consumerDir = path.join(workDir, "consumer");
  fs.mkdirSync(packDir, { recursive: true });
  fs.mkdirSync(consumerDir, { recursive: true });
  if (inside(consumerDir, repoRoot)) {
    return fail("source-tree-fallback", "scratch consumer must live outside the repository");
  }

  const units = publishedUnits({ ...options, repoRoot });
  const packed = [];
  const tarballByName = new Map();
  try {
    if (options.tarballs) {
      for (const row of options.tarballs) {
        const dest = path.join(packDir, path.basename(row.tarball));
        fs.copyFileSync(row.tarball, dest);
        const names = tarNames(dest);
        tarballByName.set(row.name, { tarball: dest, files: names, hash: sha256(dest) });
        packed.push({ name: row.name, tarball: dest, hash: sha256(dest), files: names.length });
      }
    } else {
      for (const unit of units) {
        const tarball = packUnit(unit, packDir, repoRoot);
        const names = tarNames(tarball);
        tarballByName.set(unit.name, { tarball, files: names, hash: sha256(tarball) });
        packed.push({
          name: unit.name,
          tarball,
          hash: sha256(tarball),
          files: names.length,
          kind: unit.kind,
        });
      }
    }
  } catch (error) {
    return fail("unloadable", error instanceof Error ? error.message : String(error), { packed });
  }

  const expectedNames = new Set(units.map((unit) => unit.name));
  const packedNames = new Set(tarballByName.keys());
  const uncovered = [...expectedNames].filter((name) => !packedNames.has(name));
  if (uncovered.length > 0) {
    return fail("uncovered-package", `published package(s) not packed: ${uncovered.join(", ")}`, {
      packed,
      uncovered,
    });
  }

  const installable = [];
  for (const [name, info] of tarballByName) {
    const pkg = packedManifest(info.tarball) ?? { name };
    info.pkg = pkg;
    const match = platformMatches(pkg);
    info.platformMatch = match;
    const packedIso = inspectDeps(pkg, consumerDir, repoRoot, { consumer: false });
    if (packedIso) return { ...packedIso, packed };
    if (match) installable.push(info.tarball);
  }

  fs.writeFileSync(
    path.join(consumerDir, "package.json"),
    `${JSON.stringify({ name: "rel3-clean-room-consumer", private: true, type: "module" }, null, 2)}\n`,
  );
  writeSmokeScripts(consumerDir);
  const isolated = inspectConsumer(consumerDir, repoRoot);
  if (isolated) return { ...isolated, packed };

  if (installable.length > 0) {
    const install = runNpm(
      ["install", "--ignore-scripts", "--no-package-lock", "--install-links=false", ...installable],
      { cwd: consumerDir },
    );
    if (install.status !== 0) {
      return fail(
        "unloadable",
        `npm install of packed tarballs failed:\n${install.stderr || install.stdout}`,
        { packed },
      );
    }
  }
  const afterInstall = inspectConsumer(consumerDir, repoRoot);
  if (afterInstall) return { ...afterInstall, packed };
  const installedIso = inspectInstalled(consumerDir, repoRoot);
  if (installedIso) return { ...installedIso, packed };

  const entrypoints = [];
  const failures = [];
  const loadLog = path.join(workDir, "loaded-modules.log");

  const coverBinaryPayload = (unit, info, pkg) => {
    const files = payloadFiles(pkg, info.files);
    if (files.length === 0) return false;
    if (files.some((name) => JS_FILE.test(name))) return false;
    for (const file of files) {
      if (!info.files.includes(file)) {
        failures.push({
          package: unit.name,
          entrypoint: file,
          reason: "missing-entrypoint-file",
          message: `${unit.name} payload ${file} is absent from the tarball`,
        });
        continue;
      }
      if (!info.platformMatch) {
        entrypoints.push({
          package: unit.name,
          entrypoint: file,
          condition: "packed",
          ok: true,
        });
        continue;
      }
      const installed = path.join(consumerDir, "node_modules", ...unit.name.split("/"), file);
      if (!fs.existsSync(installed)) {
        failures.push({
          package: unit.name,
          entrypoint: file,
          reason: "missing-entrypoint-file",
          message: `${unit.name} payload ${file} did not install`,
        });
        continue;
      }
      const loaded = smokeBin(installed, consumerDir);
      if (loaded?.error?.code === "ENOENT" || (loaded && loaded.status !== 0)) {
        const text = `${loaded?.stderr ?? ""}\n${loaded?.stdout ?? ""}`;
        if (/ERR_MODULE_NOT_FOUND|Cannot find module/u.test(text)) {
          failures.push({
            package: unit.name,
            entrypoint: file,
            reason: classifyLoadError(loaded.stderr, loaded.stdout),
            message: (loaded.stderr || loaded.stdout || "bin load failed").trim(),
          });
          continue;
        }
        if (loaded?.error?.code === "ENOENT") {
          failures.push({
            package: unit.name,
            entrypoint: file,
            reason: "unloadable",
            message: `${unit.name} payload ${file} could not be spawned`,
          });
          continue;
        }
        failures.push({
          package: unit.name,
          entrypoint: file,
          reason: "unloadable",
          message:
            text.trim() || `${unit.name} payload ${file} exited with status ${loaded.status}`,
        });
        continue;
      }
      entrypoints.push({
        package: unit.name,
        entrypoint: file,
        condition: "bin",
        ok: true,
      });
    }
    return true;
  };

  for (const unit of units) {
    const info = tarballByName.get(unit.name);
    const pkg = info.pkg ?? { name: unit.name };
    let declared;
    try {
      declared = declaredEntrypoints(pkg);
    } catch (error) {
      failures.push({
        package: unit.name,
        reason: "unloadable",
        message: error instanceof Error ? error.message : String(error),
      });
      continue;
    }
    const jsFiles = info.files.filter((name) => JS_FILE.test(name));
    if (declared.length === 0) {
      if (coverBinaryPayload(unit, info, pkg)) continue;
      if (jsFiles.length > 0) {
        failures.push({
          package: unit.name,
          reason: "missing-exports-condition",
          message: `${unit.name} has runtime files but no declared entrypoint`,
        });
      }
      continue;
    }
    if (declared.every((row) => row.typesOnly) && jsFiles.length > 0 && pkg.exports) {
      failures.push({
        package: unit.name,
        reason: "missing-exports-condition",
        message: `${unit.name} ships runtime files but exports declare no import/require/default condition`,
      });
      continue;
    }
    for (const row of declared) {
      if (row.typesOnly) {
        if (!info.files.includes(row.file) && row.file) {
          failures.push({
            package: unit.name,
            entrypoint: row.subpath,
            reason: "missing-entrypoint-file",
            message: `${unit.name} ${row.subpath} types file ${row.file} is absent from the tarball`,
          });
          continue;
        }
        entrypoints.push({
          package: unit.name,
          entrypoint: row.subpath,
          condition: "types",
          ok: true,
        });
        continue;
      }
      if (!info.files.includes(row.file)) {
        failures.push({
          package: unit.name,
          entrypoint: row.subpath,
          reason: "missing-entrypoint-file",
          message: `${unit.name} ${row.subpath} file ${row.file} is absent from the tarball`,
        });
        continue;
      }
      if (unit.kind === "platform" && !info.platformMatch) {
        entrypoints.push({
          package: unit.name,
          entrypoint: row.subpath,
          condition: "packed",
          ok: true,
        });
        continue;
      }
      const conditions =
        row.conditions.includes("require") && row.conditions.includes("import")
          ? ["import", "require"]
          : row.conditions.includes("require") && !row.conditions.includes("import")
            ? ["require"]
            : row.conditions.includes("bin")
              ? ["bin"]
              : ["import"];
      for (const condition of conditions) {
        const spec = specifierFor(unit.name, row.subpath);
        if (condition === "bin") {
          const binPath = path.join(consumerDir, "node_modules", ...unit.name.split("/"), row.file);
          if (!fs.existsSync(binPath)) {
            failures.push({
              package: unit.name,
              entrypoint: row.subpath,
              reason: "missing-entrypoint-file",
              message: `${unit.name} bin ${row.file} did not install`,
            });
            continue;
          }
          const loaded = smokeBin(binPath, consumerDir);
          if (loaded.status !== 0) {
            failures.push({
              package: unit.name,
              entrypoint: row.subpath,
              reason: classifyLoadError(loaded.stderr, loaded.stdout),
              message: loaded.stderr || loaded.stdout || "bin load failed",
            });
            continue;
          }
          entrypoints.push({ package: unit.name, entrypoint: row.subpath, condition, ok: true });
          continue;
        }
        const result = smoke(consumerDir, spec, condition, loadLog);
        if (result.status !== 0) {
          failures.push({
            package: unit.name,
            entrypoint: row.subpath,
            condition,
            reason: classifyLoadError(result.stderr, result.stdout),
            message: (result.stderr || result.stdout || "import failed").trim(),
          });
          continue;
        }
        let payload = {};
        try {
          payload = JSON.parse((result.stdout || "").trim().split(/\r?\n/u).at(-1));
        } catch {
          payload = {};
        }
        const resolvedRaw = payload.resolved;
        const resolved =
          typeof resolvedRaw === "string" && resolvedRaw.startsWith("file:")
            ? fileURLToPath(resolvedRaw)
            : typeof resolvedRaw === "string"
              ? resolvedRaw
              : null;
        const leaked = isolationFromLoaded(payload.loaded, resolved, consumerDir, repoRoot);
        if (leaked) {
          failures.push({
            package: unit.name,
            entrypoint: row.subpath,
            condition,
            reason: leaked.reason,
            message: leaked.message,
          });
          continue;
        }
        entrypoints.push({
          package: unit.name,
          entrypoint: row.subpath,
          condition,
          ok: true,
          resolved,
        });
      }
    }
  }

  if (failures.length > 0) {
    const primary = failures[0];
    return {
      ok: false,
      skipped: false,
      kind: CLEAN_ROOM_KIND,
      reason: primary.reason,
      message: primary.message,
      packed,
      entrypoints,
      failures,
    };
  }

  const covered = new Set(entrypoints.map((row) => row.package));
  const missingPackages = [...expectedNames].filter((name) => !covered.has(name));
  if (missingPackages.length > 0) {
    return fail(
      "uncovered-package",
      `published package(s) not smoked: ${missingPackages.join(", ")}`,
      {
        packed,
        entrypoints,
        uncovered: missingPackages,
      },
    );
  }

  return {
    ok: true,
    skipped: false,
    kind: CLEAN_ROOM_KIND,
    packed,
    entrypoints,
    workDir,
    consumerDir,
  };
}

export function runCleanRoomCheck(options = {}) {
  try {
    return runCleanRoomCheckInner(options);
  } catch (error) {
    return fail("unloadable", error instanceof Error ? error.message : String(error));
  }
}

function parseCli(argv) {
  const options = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--repo-root") {
      options.repoRoot = argv[i + 1];
      i += 1;
    } else if (arg === "--work-dir") {
      options.workDir = argv[i + 1];
      i += 1;
    } else if (arg === "--report") {
      options.report = argv[i + 1];
      i += 1;
    } else if (arg === "--skip") {
      options.skip = true;
    } else if (arg.startsWith("--")) {
      throw new Error(`unknown flag ${arg}`);
    }
  }
  return options;
}

export function main(argv = process.argv.slice(2)) {
  let parsed;
  try {
    parsed = parseCli(argv);
  } catch (error) {
    const report = fail("unloadable", error instanceof Error ? error.message : String(error));
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    return 1;
  }
  const report = runCleanRoomCheck(parsed);
  const text = `${JSON.stringify(report, null, 2)}\n`;
  process.stdout.write(text);
  if (typeof parsed.report === "string" && parsed.report.length > 0) {
    fs.mkdirSync(path.dirname(parsed.report), { recursive: true });
    fs.writeFileSync(parsed.report, text);
  }
  return report.ok ? 0 : 1;
}

if (invokedAsEntrypoint(process.argv[1], import.meta.url)) process.exitCode = main();
