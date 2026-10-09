#!/usr/bin/env node
// Provisioning of the pinned third-party tools of the opt-in comparisons
// (tools.json): each is fetched (a release file or archive, verified by its
// pinned sha256) or cloned at its pinned commit and built, into
//
//   target/oss-tools/<tool>/<pin>/
//
// outside the tracked tree, and never updated: a different pin is a
// different directory. Every build of third-party code runs under
// verter-supervise with a memory cap and a deadline (a build script is
// third-party code too). The directory ends with exactly one of
//
//   provenance.json   what was fetched or built and the binary's sha256;
//   unavailable.json  why the tool is unavailable on this platform.
//
//   node scripts/benchmark/semantic-perf/oss/provision.mjs [--tools tsz,biome] [--force]
//       [--supervisor <exe>] [--build-mem-mb 12288] [--build-timeout-ms 3600000] [--jobs <n>]

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { BUILD_ENV_NAMES, constructedEnv, sha256File } from "../provenance.mjs";
import { resolveSupervisor, runSupervised } from "../supervisor.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..", "..", "..", "..");
export const TOOLS_FILE = join(HERE, "tools.json");
export const PROVENANCE_SCHEMA = 1;

/** The pinned tool manifest. */
export function loadTools(file = TOOLS_FILE) {
  const manifest = JSON.parse(readFileSync(file, "utf8"));
  if (manifest.schema !== 1) throw new Error(`${file}: schema ${manifest.schema} is not 1`);
  return manifest.tools;
}

/** This host's platform key (`win32-x64`, `darwin-arm64`, ...). */
export const platformKey = (platform = process.platform, arch = process.arch) =>
  `${platform}-${arch}`;

/**
 * The tool's recipe for `platform`, or `{ unavailable }` when the manifest
 * has none: an exact platform entry wins over the `*` (build from source
 * anywhere) entry.
 */
export function recipeFor(tool, platform = platformKey()) {
  const recipe = tool.platforms?.[platform] ?? tool.platforms?.["*"];
  if (!recipe)
    return {
      unavailable: `no pinned release or source build for ${platform} (pinned for ${Object.keys(tool.platforms ?? {}).join(", ")})`,
    };
  return recipe;
}

/** The directory a tool's pin provisions into. */
export function toolDir(root, id, tool) {
  return join(root, "target", "oss-tools", id, tool.pin);
}

/** A path from the manifest with `{exe}` resolved for `platform`. */
export function binaryPath(dir, recipe, platform = platformKey()) {
  return join(dir, recipe.binary.replace("{exe}", platform.startsWith("win32") ? ".exe" : ""));
}

/**
 * What is provisioned for one tool, checked against its pin:
 * `{ ok, binary, sha256, provenance }` or `{ unavailable }` (with the
 * reason; never silently absent). The binary must still hash to the value
 * its provenance recorded.
 */
export function provisioned(root, id, tool, platform = platformKey()) {
  const dir = toolDir(root, id, tool);
  const recipe = recipeFor(tool, platform);
  if (recipe.unavailable)
    return { unavailable: `unavailable on ${platform}: ${recipe.unavailable}` };
  const unavailableFile = join(dir, "unavailable.json");
  if (existsSync(unavailableFile)) {
    const u = JSON.parse(readFileSync(unavailableFile, "utf8"));
    return { unavailable: `unavailable on ${u.platform ?? platform}: ${u.reason}` };
  }
  const provFile = join(dir, "provenance.json");
  if (!existsSync(provFile))
    return {
      unavailable: `unavailable on ${platform}: not provisioned (run node scripts/benchmark/semantic-perf/oss/provision.mjs --tools ${id})`,
    };
  const provenance = JSON.parse(readFileSync(provFile, "utf8"));
  const problems = provenanceProblems(provenance, id, tool, platform);
  if (problems.length) return { unavailable: `unavailable on ${platform}: ${problems.join("; ")}` };
  const binary = provenance.binary.path;
  if (!existsSync(binary))
    return { unavailable: `unavailable on ${platform}: ${binary} is missing` };
  const sha256 = sha256File(binary);
  if (sha256 !== provenance.binary.sha256)
    return {
      unavailable: `unavailable on ${platform}: ${binary} no longer hashes to its provisioned sha256`,
    };
  return { ok: true, binary, sha256, provenance };
}

/** Problems with a provenance record against the pinned manifest entry. */
export function provenanceProblems(provenance, id, tool, platform = platformKey()) {
  const problems = [];
  if (provenance?.schema !== PROVENANCE_SCHEMA)
    problems.push(`provenance schema ${provenance?.schema} is not ${PROVENANCE_SCHEMA}`);
  if (provenance?.tool !== id) problems.push(`provenance names ${provenance?.tool}, not ${id}`);
  if (provenance?.pin !== tool.pin || provenance?.commit !== tool.commit)
    problems.push(
      `provisioned ${provenance?.pin} (${provenance?.commit}), pinned ${tool.pin} (${tool.commit})`,
    );
  if (provenance?.platform !== platform)
    problems.push(`provisioned for ${provenance?.platform}, not ${platform}`);
  const recipe = recipeFor(tool, platform);
  if (JSON.stringify(provenance?.source) !== JSON.stringify(recipe.source))
    problems.push("provisioned from another source than the pinned one");
  if (typeof provenance?.binary?.sha256 !== "string") problems.push("no binary sha256 recorded");
  return problems;
}

const sha256Buffer = (buffer) => createHash("sha256").update(buffer).digest("hex");

async function download(url, sha256) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) throw new Error(`download ${url}: HTTP ${response.status}`);
  const buffer = Buffer.from(await response.arrayBuffer());
  const got = sha256Buffer(buffer);
  if (got !== sha256) throw new Error(`download ${url}: sha256 ${got}, pinned ${sha256}`);
  return buffer;
}

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { encoding: "utf8", maxBuffer: 1 << 28, ...opts });
  if (r.status !== 0)
    throw new Error(
      `${cmd} ${args.join(" ")} failed (exit ${r.status}): ${(r.stderr || r.stdout || String(r.error ?? "")).trim().slice(0, 600)}`,
    );
  return r.stdout.trim();
}

/** `rustc -vV` of the toolchain a source tree selects (its rust-toolchain file or the default). */
function toolchainIdentity(dir, env) {
  const r = spawnSync("rustc", ["-vV"], { cwd: dir, encoding: "utf8", env, timeout: 600_000 });
  return r.status === 0 ? r.stdout.trim() : null;
}

/**
 * The smoke program: one type error a checker must report on its line. The
 * library declares the global types tsc requires under `noLib`; without
 * them tsc reports only TS2318 and never checks `smoke.ts`.
 */
export const SMOKE_FILES = {
  "tsconfig.json":
    JSON.stringify(
      {
        compilerOptions: { strict: true, noLib: true, noEmit: true },
        files: ["lib.smoke.d.ts", "smoke.ts"],
      },
      null,
      2,
    ) + "\n",
  "lib.smoke.d.ts": [
    "Array<T>",
    "Boolean",
    "CallableFunction",
    "Function",
    "IArguments",
    "NewableFunction",
    "Number",
    "Object",
    "RegExp",
    "String",
  ]
    .map((name) => `interface ${name} {}\n`)
    .join(""),
  "smoke.ts": "export const x: string = 1;\n",
};

/**
 * Whether the tool runs on this platform at all: its smoke command (under
 * the supervisor, with the benchmark's default cap) must exit normally and
 * print the manifest's expected text. A tool that cannot check a one-line
 * program here is unavailable here; what it answers on the benchmark's
 * programs is a finding of the run, never decided here.
 */
async function smokeCheck(root, dir, binary, tool, options) {
  if (!tool.smoke) return { ok: true, detail: "no smoke check" };
  const smokeDir = join(dir, "smoke");
  mkdirSync(smokeDir, { recursive: true });
  for (const [name, text] of Object.entries(SMOKE_FILES)) writeFileSync(join(smokeDir, name), text);
  const argv = [
    binary,
    ...tool.smoke.argv.map((a) =>
      a
        .replace("{tsconfig}", join(smokeDir, "tsconfig.json"))
        .replace("{scenario}", join(smokeDir, "smoke.ts")),
    ),
  ];
  const supervisor = options.supervisor ?? resolveSupervisor(root, null).path;
  const out = join(smokeDir, "smoke.sup.json");
  const r = await runSupervised(supervisor, {
    memMb: 9216,
    timeoutMs: 60_000,
    out,
    cwd: smokeDir,
    argv,
    allowSampled: process.platform === "darwin",
  });
  const rec = r.record;
  const text = [rec?.stdoutPath, rec?.stderrPath]
    .filter((p) => p && existsSync(p))
    .map((p) => readFileSync(p, "utf8"))
    .join("\n")
    .replace(/\x1b\[[0-9;]*m/g, "");
  const first =
    text
      .split(/\r?\n/)
      .map((l) => l.trim())
      .find(Boolean) ?? "";
  const detail = !rec?.launched
    ? `not launched (${(rec?.errors ?? []).join("; ") || r.readError || r.spawnError})`
    : rec.killedBy
      ? `killed (${rec.killedBy}) on a one-line program`
      : ![0, 1, 2].includes(rec.exitCode)
        ? `exit ${rec.exitCode} on a one-line program${first ? `: ${first.slice(0, 200)}` : ""}`
        : text.includes(tool.smoke.expect)
          ? "ok"
          : `a one-line program's error was not reported (${first ? `output: ${first.slice(0, 200)}` : "no output"})`;
  return { ok: detail === "ok", detail, argv, exitCode: rec?.exitCode ?? null };
}

/**
 * Provision one tool. Returns what was written (`provenance` or
 * `unavailable`). A tool already provisioned at its pin is kept unless
 * `force`.
 */
export async function provisionTool(root, id, tool, options = {}) {
  const platform = platformKey();
  const dir = toolDir(root, id, tool);
  const log = options.log ?? console.log;
  const existing = provisioned(root, id, tool, platform);
  if (existing.ok && !options.force) {
    log(`${id}: provisioned (${tool.pin}, ${existing.sha256.slice(0, 12)})`);
    return { provenance: existing.provenance };
  }
  const recipe = recipeFor(tool, platform);
  const unavailable = (reason, extra = {}) => {
    mkdirSync(dir, { recursive: true });
    const record = {
      schema: PROVENANCE_SCHEMA,
      tool: id,
      pin: tool.pin,
      platform,
      reason,
      ...extra,
    };
    writeFileSync(join(dir, "unavailable.json"), JSON.stringify(record, null, 2) + "\n");
    rmSync(join(dir, "provenance.json"), { force: true });
    log(`${id}: unavailable on ${platform}: ${reason}`);
    return { unavailable: record };
  };
  if (recipe.unavailable) return unavailable(recipe.unavailable);
  mkdirSync(dir, { recursive: true });
  rmSync(join(dir, "unavailable.json"), { force: true });
  const source = recipe.source;
  let build = null;
  try {
    if (source.type === "file" || source.type === "archive") {
      log(`${id}: fetching ${source.url}`);
      const buffer = await download(source.url, source.sha256);
      const name = decodeURIComponent(source.url.split("/").at(-1));
      const file = join(dir, name);
      writeFileSync(`${file}.tmp`, buffer);
      renameSync(`${file}.tmp`, file);
      // A release file is the executable itself: a download carries no mode bits.
      if (source.type === "file" && process.platform !== "win32") chmodSync(file, 0o755);
      // bsdtar (the system tar of Windows and macOS) reads both .zip and
      // .tar.gz; on Windows it is named by path, since a GNU tar earlier on
      // PATH reads no zip.
      const tar =
        process.platform === "win32"
          ? join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe")
          : "tar";
      if (source.type === "archive") run(tar, ["-xf", name], { cwd: dir });
    } else if (source.type === "git") {
      const src = dir;
      log(`${id}: fetching ${source.url} at ${source.commit}`);
      if (!existsSync(join(src, ".git"))) run("git", ["init", "-q", src]);
      run("git", ["-C", src, "fetch", "-q", "--depth", "1", source.url, source.commit]);
      run("git", ["-C", src, "checkout", "-q", "--detach", "FETCH_HEAD"]);
      const head = run("git", ["-C", src, "rev-parse", "HEAD"]);
      if (head !== source.commit) throw new Error(`checked out ${head}, pinned ${source.commit}`);
      const supervisor = options.supervisor ?? resolveSupervisor(root, null).path;
      const jobs = options.jobs ?? process.env.CARGO_BUILD_JOBS ?? null;
      const env = constructedEnv(BUILD_ENV_NAMES, {
        CARGO_INCREMENTAL: "0",
        CARGO_TARGET_DIR: join(src, "target"),
        ...(jobs ? { CARGO_BUILD_JOBS: String(jobs) } : {}),
      });
      const rustc = toolchainIdentity(src, env);
      log(`${id}: building (${recipe.build.join(" ")}) under the supervisor`);
      const out = join(dir, "build.sup.json");
      const result = await runSupervised(supervisor, {
        memMb: options.buildMemMb ?? 12288,
        timeoutMs: options.buildTimeoutMs ?? 3_600_000,
        out,
        cwd: src,
        env,
        argv: recipe.build,
        allowSampled: process.platform === "darwin",
      });
      const rec = result.record;
      build = {
        argv: recipe.build,
        rustc,
        envNames: Object.keys(env).sort(),
        supervisor: rec
          ? {
              exitCode: rec.exitCode,
              killedBy: rec.killedBy,
              wallMs: rec.wallMs,
              peakBytes: rec.peakBytes,
              containment: rec.containment,
              errors: rec.errors,
            }
          : null,
        log: rec?.stderrPath ?? null,
      };
      if (!rec?.launched || rec.killedBy || rec.exitCode !== 0 || (rec.errors ?? []).length) {
        const tail =
          rec?.stderrPath && existsSync(rec.stderrPath)
            ? readFileSync(rec.stderrPath, "utf8")
                .split(/\r?\n/)
                .filter((l) => /error/i.test(l))
                .slice(-3)
                .join(" / ")
            : "";
        return unavailable(
          `the pinned build failed (${rec?.killedBy ? `killed: ${rec.killedBy}` : `exit ${rec?.exitCode ?? "?"}`})${tail ? `: ${tail.slice(0, 400)}` : ""}`,
          { build },
        );
      }
    } else throw new Error(`unknown source type ${source.type}`);
  } catch (err) {
    return unavailable(String(err.message ?? err).slice(0, 800), { build });
  }
  const binary = binaryPath(dir, recipe, platform);
  if (!existsSync(binary))
    return unavailable(`the binary ${binary} is missing after provisioning`, { build });
  const smoke = await smokeCheck(root, dir, binary, tool, options);
  if (!smoke.ok) return unavailable(`it does not run here: ${smoke.detail}`, { build, smoke });
  const provenance = {
    schema: PROVENANCE_SCHEMA,
    tool: id,
    name: tool.name,
    project: tool.project,
    license: tool.license,
    pin: tool.pin,
    commit: tool.commit,
    platform,
    source,
    build,
    smoke,
    binary: { path: binary, sha256: sha256File(binary) },
    provisionedAt: new Date().toISOString(),
  };
  writeFileSync(join(dir, "provenance.json"), JSON.stringify(provenance, null, 2) + "\n");
  log(`${id}: provisioned ${binary} (${provenance.binary.sha256.slice(0, 12)})`);
  return { provenance };
}

async function main(argv) {
  const value = (name) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const tools = loadTools();
  const ids = (value("--tools") ?? Object.keys(tools).join(",")).split(",").filter(Boolean);
  for (const id of ids)
    if (!tools[id]) throw new Error(`unknown tool ${id}; tools: ${Object.keys(tools).join(", ")}`);
  const supervisor = value("--supervisor")
    ? resolve(value("--supervisor"))
    : resolveSupervisor(ROOT, null).path;
  let failed = 0;
  for (const id of ids) {
    const r = await provisionTool(ROOT, id, tools[id], {
      force: argv.includes("--force"),
      supervisor,
      buildMemMb: value("--build-mem-mb") ? Number(value("--build-mem-mb")) : undefined,
      buildTimeoutMs: value("--build-timeout-ms") ? Number(value("--build-timeout-ms")) : undefined,
      jobs: value("--jobs"),
    });
    if (r.unavailable) failed++;
  }
  return failed ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (err) => {
      console.error(`provision: ${err?.message ?? err}`);
      process.exit(2);
    },
  );
}
