// Provenance of one benchmark run: the source tree, the host, the pinned
// TypeScript 7.0.2 packages, the Verter probe binaries and the supervisor —
// each identified by content, so the validator can prove the run measured
// the artifacts it names.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import { basename, dirname, join, relative } from "node:path";

export const TYPESCRIPT_VERSION = "7.0.2";

/** Features no production build enables; a probe built with one is not the production library. */
export const FORBIDDEN_FEATURES = {
  verter_session: [
    "test-support",
    "test-util",
    "oracle-gen",
    "oracle-lift",
    "attribution",
    "currency_probe",
  ],
  verter_audit: ["attribution"],
  verter_workspace: ["currency_probe"],
  verter_bench: ["attribution", "currency_probe", "hotpath", "hotpath-alloc"],
  verter_scheduler: ["test-support"],
};

/**
 * The features the observe build enables: `semantic-observe` and what it
 * implies. Only that build may carry them, and it must carry the gate.
 */
export const OBSERVE_FEATURES = {
  verter_bench: ["semantic-observe", "attribution", "currency_probe", "hotpath"],
  verter_audit: ["semantic-observe", "attribution"],
  verter_session: ["currency_probe"],
  verter_workspace: ["semantic-observe", "currency_probe"],
  verter_semantic: ["semantic-observe"],
  verter_compiler: ["semantic-observe"],
  verter_scheduler: ["semantic-observe"],
};

/** Packages whose build features and profile are recorded. */
export const RECORDED_PACKAGES = [
  "verter_bench",
  "verter_session",
  "verter_semantic",
  "verter_workspace",
  "verter_audit",
  "verter_type_expr",
  "verter_scheduler",
  "verter_compiler",
];

export function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function sha256Text(text) {
  return createHash("sha256").update(text).digest("hex");
}

function git(root, args) {
  const r = spawnSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 1 << 30 });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")} failed: ${r.stderr}`);
  return r.stdout;
}

/** The paths the Verter probe binaries are built from. */
export const BUILD_INPUTS = ["crates", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo"];

/**
 * The source tree at HEAD plus the exact uncommitted delta, over the whole
 * checkout or, with `pathspec`, over those paths only.
 */
export function sourceTree(root, pathspec = []) {
  const head = git(root, ["rev-parse", "HEAD"]).trim();
  const branch = git(root, ["rev-parse", "--abbrev-ref", "HEAD"]).trim();
  const scope = pathspec.length ? ["--", ...pathspec] : [];
  const status = git(root, ["status", "--porcelain=v1", "--untracked-files=all", ...scope]);
  const diff = git(root, ["diff", "HEAD", "--binary", ...scope]);
  const untracked = status
    .split("\n")
    .filter((line) => line.startsWith("?? "))
    .map((line) => line.slice(3).trim())
    .sort();
  const untrackedDigest = createHash("sha256");
  for (const path of untracked) {
    untrackedDigest.update(path + "\0");
    try {
      untrackedDigest.update(readFileSync(join(root, path)));
    } catch {
      untrackedDigest.update("<unreadable>");
    }
  }
  return {
    head,
    branch,
    dirty: status.trim().length > 0,
    changedPaths: status.split("\n").filter(Boolean).length,
    diffSha256: sha256Text(diff),
    untrackedSha256: untrackedDigest.digest("hex"),
  };
}

/**
 * sha256 of every file of the harness itself (this directory and its CLI):
 * the methodology a run used. tsc-probe.mjs is re-read by every invocation,
 * so the harness must not change during a run.
 */
export function harnessFingerprint(root) {
  const dir = join(root, "scripts", "benchmark", "semantic-perf");
  const files = [join(root, "scripts", "benchmark", "semantic-perf.mjs")];
  const walk = (d) => {
    for (const entry of readdirSync(d, { withFileTypes: true })) {
      const path = join(d, entry.name);
      if (entry.isDirectory()) walk(path);
      else files.push(path);
    }
  };
  walk(dir);
  return Object.fromEntries(
    files
      .map((f) => [relative(root, f).split("\\").join("/"), sha256File(f)])
      .sort(([a], [b]) => a.localeCompare(b)),
  );
}

export function hostInfo() {
  const cpus = os.cpus();
  return {
    platform: process.platform,
    arch: process.arch,
    osRelease: os.release(),
    osVersion: typeof os.version === "function" ? os.version() : null,
    cpuModel: cpus[0]?.model ?? null,
    logicalCpus: cpus.length,
    totalMemoryBytes: os.totalmem(),
    node: process.version,
    nodeExe: process.execPath,
  };
}

/**
 * Resolve the pinned TypeScript packages through ordinary dependency
 * resolution from `fromDir` (a directory whose node_modules holds the
 * repository's `typescript` devDependency) and prove their version.
 */
export function resolveTypeScript(fromDir) {
  const requireFrom = createRequire(join(fromDir, "package.json"));
  let tsPackageJson;
  try {
    tsPackageJson = requireFrom.resolve("typescript/package.json");
  } catch {
    throw new Error(
      `cannot resolve \`typescript\` from ${fromDir}; run \`pnpm install\` at the repository root or pass --typescript-from <dir>`,
    );
  }
  const tsPackageDir = dirname(tsPackageJson);
  const tsPackage = JSON.parse(readFileSync(tsPackageJson, "utf8"));
  if (tsPackage.version !== TYPESCRIPT_VERSION) {
    throw new Error(
      `typescript at ${tsPackageDir} is ${tsPackage.version}, not the pinned ${TYPESCRIPT_VERSION}`,
    );
  }
  const platformName = `@typescript/typescript-${process.platform}-${process.arch}`;
  const requireTs = createRequire(tsPackageJson);
  let platformJson;
  try {
    platformJson = requireTs.resolve(`${platformName}/package.json`);
  } catch {
    throw new Error(
      `cannot resolve ${platformName} from ${tsPackageDir}: the native tsc for this platform is not installed`,
    );
  }
  const platformDir = dirname(platformJson);
  const platformPackage = JSON.parse(readFileSync(platformJson, "utf8"));
  if (platformPackage.version !== TYPESCRIPT_VERSION) {
    throw new Error(
      `${platformName} is ${platformPackage.version}, not the pinned ${TYPESCRIPT_VERSION}`,
    );
  }
  const exe = join(platformDir, "lib", process.platform === "win32" ? "tsc.exe" : "tsc");
  if (!existsSync(exe)) throw new Error(`the native tsc is missing at ${exe}`);
  const version = spawnSync(exe, ["-v"], { encoding: "utf8", timeout: 30_000 });
  const versionText = (version.stdout ?? "").trim();
  if (version.status !== 0 || versionText !== `Version ${TYPESCRIPT_VERSION}`) {
    throw new Error(
      `${exe} -v answered ${JSON.stringify(versionText)} (exit ${version.status}), not Version ${TYPESCRIPT_VERSION}`,
    );
  }
  const apiFiles = [
    "dist/api/sync/api.js",
    "dist/api/sync/client.js",
    "dist/api/syncChannel.js",
    "dist/api/async/api.js",
    "dist/api/async/client.js",
  ];
  return {
    packageDir: tsPackageDir,
    version: tsPackage.version,
    gitHead: tsPackage.gitHead ?? null,
    platformPackage: platformName,
    platformVersion: platformPackage.version,
    exe,
    exeSha256: sha256File(exe),
    versionText,
    apiSha256: Object.fromEntries(apiFiles.map((f) => [f, sha256File(join(tsPackageDir, f))])),
  };
}

/**
 * Build the Verter probe binaries in the production release profile and
 * return what cargo reports about them: executables, and the profile and
 * features of every recorded package.
 */
/**
 * The variables a child process needs to start and find its files, and
 * nothing else: every environment the harness gives a child (the build, the
 * supervisor and through it every probe) is constructed from this list, not
 * inherited, so no allocator, runtime or compiler setting of the caller's
 * shell reaches a measurement. Names are compared case-insensitively (as
 * Windows does).
 */
export const RUNTIME_ENV_NAMES = [
  // every platform
  "PATH",
  "HOME",
  "TMPDIR",
  "TMP",
  "TEMP",
  "USER",
  "LOGNAME",
  // Windows
  "PATHEXT",
  "SYSTEMROOT",
  "SYSTEMDRIVE",
  "WINDIR",
  "COMSPEC",
  "USERPROFILE",
  "USERNAME",
  "HOMEDRIVE",
  "HOMEPATH",
  "APPDATA",
  "LOCALAPPDATA",
  "PROGRAMDATA",
  "PROGRAMFILES",
  "PROGRAMFILES(X86)",
  "PROGRAMW6432",
  "COMMONPROGRAMFILES",
  "COMMONPROGRAMFILES(X86)",
  "COMMONPROGRAMW6432",
  "NUMBER_OF_PROCESSORS",
  "PROCESSOR_ARCHITECTURE",
  "OS",
];
/** The build's additions: where the toolchain lives. */
export const BUILD_ENV_NAMES = [...RUNTIME_ENV_NAMES, "CARGO_HOME", "RUSTUP_HOME"];

/**
 * An environment built from `names` (taken from `source`) plus `set`.
 * With `inherit` (a labelled tuned run) the whole source is passed on.
 */
export function constructedEnv(names, set = {}, { source = process.env, inherit = false } = {}) {
  const wanted = new Set(names.map((n) => n.toUpperCase()));
  const setNames = new Set(Object.keys(set).map((n) => n.toUpperCase()));
  const env = Object.fromEntries(
    Object.entries(source).filter(
      ([k, v]) =>
        v !== undefined &&
        !setNames.has(k.toUpperCase()) &&
        (inherit || wanted.has(k.toUpperCase())),
    ),
  );
  return { ...env, ...set };
}

/** What an environment holds, for the record: its names and a digest of its values. */
export function envReceipt(env) {
  const names = Object.keys(env).sort((a, b) => a.localeCompare(b));
  return { names, valuesSha256: sha256Text(JSON.stringify(names.map((n) => [n, env[n]]))) };
}

/** The repository's pinned toolchain channel (rust-toolchain.toml), or null. */
export function toolchainPin(root) {
  try {
    const text = readFileSync(join(root, "rust-toolchain.toml"), "utf8");
    return text.match(/^\s*channel\s*=\s*"([^"]+)"/m)?.[1] ?? null;
  } catch {
    return null;
  }
}

/**
 * With `observe`, build only the plain probe with `semantic-observe` (optional
 * capture compiled in), in its own target directory so the production
 * artifacts are never rebuilt with the gate unified into them.
 */
export function buildVerterProbes(root, { inherit = false, observe = false } = {}) {
  const bins = observe
    ? ["semantic_perf_probe"]
    : ["semantic_perf_probe", "semantic_perf_probe_counted"];
  const args = [
    "build",
    "--release",
    "-p",
    "verter_bench",
    ...bins.flatMap((b) => ["--bin", b]),
    ...(observe
      ? [
          "--features",
          "semantic-observe",
          "--target-dir",
          join(root, "target", "semantic-perf-observe"),
        ]
      : []),
    "--message-format=json-render-diagnostics",
  ];
  // A controlled build environment: cargo is bound to the toolchain's own
  // compiler (fingerprinted below) and incremental compilation is off,
  // whatever the caller's environment says (names compared case-insensitively,
  // as Windows does).
  // The toolchain is the repository's pin, named explicitly (RUSTUP_TOOLCHAIN
  // outranks every other rustup override), and the compiler is discovered in
  // that same constructed environment, so nothing of the caller's selects it.
  const pin = toolchainPin(root);
  const discovery = constructedEnv(BUILD_ENV_NAMES, pin ? { RUSTUP_TOOLCHAIN: pin } : {}, {
    inherit,
  });
  const rustcPath =
    (
      spawnSync("rustup", ["which", "rustc"], { cwd: root, encoding: "utf8", env: discovery })
        .stdout ?? ""
    ).trim() || null;
  const controlled = {
    CARGO_INCREMENTAL: "0",
    ...(pin ? { RUSTUP_TOOLCHAIN: pin } : {}),
    ...(rustcPath ? { RUSTC: rustcPath } : {}),
  };
  const env = constructedEnv(BUILD_ENV_NAMES, controlled, { inherit });
  const r = spawnSync("cargo", args, {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 1 << 30,
    env,
    stdio: ["ignore", "pipe", "inherit"],
  });
  if (r.status !== 0) throw new Error(`cargo ${args.join(" ")} failed (exit ${r.status})`);
  const executables = {};
  const packages = {};
  for (const line of r.stdout.split("\n")) {
    if (!line.startsWith("{")) continue;
    const msg = JSON.parse(line);
    if (msg.reason !== "compiler-artifact") continue;
    const name = msg.target?.name;
    const pkgName = msg.package_id?.match(/([A-Za-z0-9_-]+)(?:@|#)[^#@]*$/)?.[1] ?? null;
    if (
      msg.executable &&
      (name === "semantic_perf_probe" || name === "semantic_perf_probe_counted")
    ) {
      executables[name] = msg.executable;
    }
    const lib = msg.target?.kind?.some((k) => k === "lib" || k === "rlib");
    const recorded = RECORDED_PACKAGES.find((p) => name === p || pkgName === p);
    if (recorded && (lib || recorded === "verter_bench")) {
      packages[recorded] = {
        features: [...msg.features].sort(),
        profile: msg.profile,
      };
    }
  }
  for (const name of bins) {
    if (!executables[name]) throw new Error(`cargo reported no executable for ${name}`);
  }
  const tool = (cmd, toolArgs) => {
    const out = spawnSync(cmd, toolArgs, { cwd: root, encoding: "utf8", env });
    return out.status === 0 ? out.stdout.trim() : null;
  };
  return {
    cargoArgs: args,
    observe,
    executables,
    packages,
    env: controlled,
    environment: envReceipt(env),
    toolchainPin: pin,
    rustc: rustcPath ? tool(rustcPath, ["-vV"]) : null,
    // The compiler cargo was bound to (RUSTC), identified by content.
    rustcPath,
    rustcSha256: (() => {
      try {
        return rustcPath ? sha256File(rustcPath) : null;
      } catch {
        return null;
      }
    })(),
    cargo: tool("cargo", ["-V"]),
  };
}

/**
 * Check a cargo build record against the production-library requirements;
 * returns problems. The observe build (`build.observe`) may carry only the
 * features `semantic-observe` implies, and must carry the gate itself.
 */
export function buildProblems(build) {
  const problems = [];
  const allowed = (name) => (build.observe ? (OBSERVE_FEATURES[name] ?? []) : []);
  if (build.observe) {
    for (const name of ["verter_bench", "verter_audit"])
      if (!build.packages?.[name]?.features?.includes("semantic-observe"))
        problems.push(`${name} of the observe build lacks semantic-observe`);
  }
  for (const name of RECORDED_PACKAGES) {
    const pkg = build.packages[name];
    if (!pkg) {
      problems.push(`cargo reported no build record for ${name}`);
      continue;
    }
    if (String(pkg.profile?.opt_level) !== "3")
      problems.push(`${name} built at opt-level ${pkg.profile?.opt_level}, not 3`);
    if (pkg.profile?.debug_assertions) problems.push(`${name} built with debug assertions`);
    if (pkg.profile?.test) problems.push(`${name} built as a test target`);
    if (!build.observe && pkg.features.includes("semantic-observe"))
      problems.push(`${name} built with optional semantic capture (semantic-observe)`);
    for (const feature of FORBIDDEN_FEATURES[name] ?? []) {
      if (pkg.features.includes(feature) && !allowed(name).includes(feature))
        problems.push(`${name} built with the non-production feature ${feature}`);
    }
  }
  return problems;
}

/** Copy `path` into `dir` under a content-addressed name and return its identity. */
export function pinBinary(path, dir, label) {
  mkdirSync(dir, { recursive: true });
  const sha256 = sha256File(path);
  const ext = process.platform === "win32" ? ".exe" : "";
  const pinned = join(dir, `${label}-${sha256.slice(0, 16)}${ext}`);
  if (!existsSync(pinned)) copyFileSync(path, pinned);
  if (sha256File(pinned) !== sha256)
    throw new Error(`the pinned copy of ${basename(path)} differs from its source`);
  return { source: path, pinned, sha256 };
}

/** Run `<probe> identity` and parse it. */
export function probeIdentity(exe) {
  const r = spawnSync(exe, ["identity"], { encoding: "utf8", timeout: 30_000 });
  if (r.status !== 0) throw new Error(`${exe} identity failed: ${r.stderr}`);
  return JSON.parse(r.stdout);
}
