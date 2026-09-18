#!/usr/bin/env node
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync } from "node:zlib";

import { GitHubAdapterError } from "./errors.mjs";
import { computePublishSet, scanWorkspacePackages } from "../lib/publish-set.mjs";

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
const TAR_BLOCK = 512;

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

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    shell: process.platform === "win32",
    ...options,
  });
  return result;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function sha256(filePath) {
  return createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

function tarNames(tgzPath) {
  const tar = gunzipSync(fs.readFileSync(tgzPath));
  const names = [];
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
      if (relative && typeflag !== "5") names.push(relative.replace(/\\/g, "/"));
    }
    offset += TAR_BLOCK + Math.ceil(size / TAR_BLOCK) * TAR_BLOCK;
  }
  return names;
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
    const key = `${subpath}\0${conditions.slice().sort().join(",")}\0${file}\0${typesOnly ? "t" : "r"}`;
    if (seen.has(key)) return;
    seen.add(key);
    rows.push({
      subpath,
      file: normalizeFile(file),
      conditions: conditions.slice(),
      typesOnly,
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

export function assertCleanRoomHosted(releaseYml) {
  const jobs = splitWorkflowJobs(releaseYml);
  const hosts = Object.entries(jobs).filter(([, body]) => body.includes(CLEAN_ROOM_SCRIPT));
  if (hosts.length === 0) {
    throw new GitHubAdapterError("release.yml must run the clean-room published-artifact check");
  }
  for (const [name, body] of hosts) {
    if (/dry-run\s*!=\s*'true'/u.test(body) || /outputs\.dry-run\s*!=\s*'true'/u.test(body)) {
      throw new GitHubAdapterError(`clean-room check in job ${name} is skipped during rehearsal`);
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

function packPackage(packageDir, packDir) {
  fs.mkdirSync(packDir, { recursive: true });
  const before = new Set(fs.readdirSync(packDir));
  const pnpm = run("pnpm", ["pack", "--pack-destination", packDir, "--json"], { cwd: packageDir });
  if (pnpm.status === 0) {
    const start = (pnpm.stdout ?? "").indexOf("{");
    if (start >= 0) {
      const info = JSON.parse(pnpm.stdout.slice(start));
      const filename = info.filename ?? info.path;
      if (filename) {
        const packed = path.isAbsolute(filename)
          ? filename
          : path.join(packDir, path.basename(filename));
        if (fs.existsSync(packed)) return packed;
      }
    }
  }
  const npm = run("npm", ["pack", "--pack-destination", packDir], { cwd: packageDir });
  if (npm.status !== 0) {
    throw new Error(
      `pack failed in ${packageDir}: ${npm.stderr || pnpm.stderr || npm.stdout || pnpm.stdout}`,
    );
  }
  const after = fs
    .readdirSync(packDir)
    .filter((name) => name.endsWith(".tgz") && !before.has(name));
  if (after.length === 0) throw new Error(`pack produced no tarball in ${packageDir}`);
  return path.join(packDir, after[0]);
}

function platformMatches(pkg) {
  if (Array.isArray(pkg.os) && pkg.os.length > 0 && !pkg.os.includes(process.platform))
    return false;
  if (Array.isArray(pkg.cpu) && pkg.cpu.length > 0 && !pkg.cpu.includes(process.arch)) return false;
  return true;
}

function writeSmokeScripts(consumerDir) {
  fs.writeFileSync(
    path.join(consumerDir, "smoke-import.mjs"),
    `const spec = process.argv[2];
const ns = await import(spec);
if (typeof ns.cleanRoomPing === "function") ns.cleanRoomPing();
else if (ns.default == null && Object.keys(ns).length === 0) throw new Error("empty module namespace");
const resolved = import.meta.resolve(spec);
console.log(JSON.stringify({ ok: true, resolved }));
`,
  );
  fs.writeFileSync(
    path.join(consumerDir, "smoke-require.cjs"),
    `"use strict";
const spec = process.argv[2];
const mod = require(spec);
if (mod && typeof mod.cleanRoomPing === "function") mod.cleanRoomPing();
else if (mod == null) throw new Error("empty module namespace");
console.log(JSON.stringify({ ok: true, resolved: require.resolve(spec) }));
`,
  );
}

function smoke(consumerDir, spec, condition) {
  const script = condition === "require" ? "smoke-require.cjs" : "smoke-import.mjs";
  const env = { ...process.env, NODE_PATH: "" };
  delete env.NODE_OPTIONS;
  const result = spawnSync(process.execPath, [path.join(consumerDir, script), spec], {
    cwd: consumerDir,
    encoding: "utf8",
    env,
    timeout: 30_000,
  });
  return result;
}

function inspectConsumer(consumerDir, repoRoot) {
  const manifestPath = path.join(consumerDir, "package.json");
  const manifest = readJson(manifestPath);
  if (manifest.patchedDependencies || manifest.pnpm?.patchedDependencies) {
    return fail("consumer-patch", "consumer-side patch cannot satisfy clean-room evidence");
  }
  if (manifest.pnpm?.overrides || manifest.overrides) {
    return fail("workspace-resolved", "lockfile override cannot satisfy clean-room evidence");
  }
  const deps = { ...manifest.dependencies, ...manifest.optionalDependencies };
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
  if (fs.existsSync(path.join(consumerDir, "pnpm-workspace.yaml"))) {
    return fail("workspace-resolved", "consumer is a pnpm workspace");
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

export function runCleanRoomCheck(options = {}) {
  if (options.skip === true) {
    return fail("skipped", "a rehearsal cannot report PASS with the clean-room check skipped");
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
        const tarball = packPackage(unit.dir, packDir);
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
    const unit = units.find((row) => row.name === name);
    let pkg = { name };
    if (unit?.dir && fs.existsSync(path.join(unit.dir, "package.json"))) {
      pkg = readJson(path.join(unit.dir, "package.json"));
    } else {
      const extractDir = path.join(workDir, "manifests", name.replaceAll("/", "-"));
      fs.mkdirSync(extractDir, { recursive: true });
      const extract = run("tar", [
        "-xzf",
        info.tarball,
        "-C",
        extractDir,
        "--strip-components",
        "1",
        "package/package.json",
      ]);
      const manifestPath = path.join(extractDir, "package.json");
      if (extract.status === 0 && fs.existsSync(manifestPath)) pkg = readJson(manifestPath);
    }
    info.pkg = pkg;
    const match = platformMatches(pkg);
    info.platformMatch = match;
    if (match) installable.push(info.tarball);
  }

  fs.writeFileSync(
    path.join(consumerDir, "package.json"),
    `${JSON.stringify({ name: "rel3-clean-room-consumer", private: true, type: "module" }, null, 2)}\n`,
  );
  writeSmokeScripts(consumerDir);
  const isolated = inspectConsumer(consumerDir, repoRoot);
  if (isolated) return { ...isolated, packed };

  const install = run(
    "npm",
    ["install", "--ignore-scripts", "--no-package-lock", "--install-links=false", ...installable],
    { cwd: consumerDir },
  );
  if (install.status !== 0) {
    return fail(
      "unloadable",
      `npm install of packed tarballs failed:\n${install.stderr || install.stdout}`,
      {
        packed,
      },
    );
  }
  const afterInstall = inspectConsumer(consumerDir, repoRoot);
  if (afterInstall) return { ...afterInstall, packed };

  const entrypoints = [];
  const failures = [];
  for (const unit of units) {
    const info = tarballByName.get(unit.name);
    const pkg = info.pkg ?? { name: unit.name };
    const declared = declaredEntrypoints(pkg);
    const jsFiles = info.files.filter((name) => JS_FILE.test(name));
    if (declared.every((row) => row.typesOnly) && jsFiles.length > 0 && pkg.exports) {
      failures.push({
        package: unit.name,
        reason: "missing-exports-condition",
        message: `${unit.name} ships runtime files but exports declare no import/require/default condition`,
      });
      continue;
    }
    if (declared.length === 0 && jsFiles.length > 0) {
      failures.push({
        package: unit.name,
        reason: "missing-exports-condition",
        message: `${unit.name} has runtime files but no declared entrypoint`,
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
          const loaded = spawnSync(process.execPath, ["--check", binPath], {
            cwd: consumerDir,
            encoding: "utf8",
            env: { ...process.env, NODE_PATH: "" },
            timeout: 15_000,
          });
          if (loaded.status !== 0) {
            failures.push({
              package: unit.name,
              entrypoint: row.subpath,
              reason: classifyLoadError(loaded.stderr, loaded.stdout),
              message: loaded.stderr || loaded.stdout || "bin parse failed",
            });
            continue;
          }
          entrypoints.push({ package: unit.name, entrypoint: row.subpath, condition, ok: true });
          continue;
        }
        const result = smoke(consumerDir, spec, condition);
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
        if (resolved && !inside(resolved, consumerDir)) {
          failures.push({
            package: unit.name,
            entrypoint: row.subpath,
            condition,
            reason: "source-tree-fallback",
            message: `${spec} resolved outside the scratch consumer: ${resolved}`,
          });
          continue;
        }
        if (resolved && inside(resolved, repoRoot) && !inside(resolved, consumerDir)) {
          failures.push({
            package: unit.name,
            entrypoint: row.subpath,
            condition,
            reason: "source-tree-fallback",
            message: `${spec} resolved into the repository`,
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
    } else if (arg === "--skip") {
      options.skip = true;
    } else if (arg.startsWith("--")) {
      throw new Error(`unknown flag ${arg}`);
    }
  }
  return options;
}

export function main(argv = process.argv.slice(2)) {
  const report = runCleanRoomCheck(parseCli(argv));
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  return report.ok ? 0 : 1;
}

const invoked = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invoked) process.exitCode = main();
