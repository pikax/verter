// Reports the health of the Rust build target directory:
//   1. On-disk size of the cargo target directory (recursive stat walk, no `du`).
//   2. (opt-in) Current nextest test-binary count + summed on-disk size.
//   3. Stale dep-executable accumulation in <target>/debug/deps.
//
// Read-only reporting tool. It never mutates the repository. With --with-binaries
// it invokes `cargo nextest list` to learn the CURRENT test-binary set, which
// triggers a cargo build if artifacts are cold (see --help).
//
// Cross-platform: walks the tree with node:fs only (no Unix `du`); the
// executable-classification branch keys on process.platform === "win32".
import { spawnSync } from "node:child_process";
import { readdirSync, statSync } from "node:fs";
import { join, resolve, extname, sep } from "node:path";
import { fileURLToPath } from "node:url";

const IS_WIN = process.platform === "win32";
const REPO_ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));

// ---------------------------------------------------------------------------
// Arg parsing
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const opts = {
    targetDir: null,
    withBinaries: false,
    json: false,
    help: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    switch (a) {
      case "-h":
      case "--help":
        opts.help = true;
        break;
      case "--with-binaries":
        opts.withBinaries = true;
        break;
      case "--json":
        opts.json = true;
        break;
      case "--target-dir":
        opts.targetDir = argv[++i];
        if (opts.targetDir == null) {
          fail(`--target-dir requires a path argument`);
        }
        break;
      default:
        if (a.startsWith("--target-dir=")) {
          opts.targetDir = a.slice("--target-dir=".length);
        } else {
          fail(`unknown argument: ${a}\nRun with --help for usage.`);
        }
    }
  }
  return opts;
}

function fail(msg) {
  console.error(`target-health: ${msg}`);
  process.exit(2);
}

const HELP = `target-health.mjs — report Rust build-target hygiene

USAGE
  node scripts/target-health.mjs [options]

OPTIONS
  --target-dir <path>   Cargo target directory to inspect.
                        Default: $CARGO_TARGET_DIR, else <repo>/target.
  --with-binaries       Additionally run \`cargo nextest list\` to learn the
                        CURRENT test-binary set, report its count + summed size,
                        and compute the STALE dep-executable count
                        (total dep executables - current test binaries).
                        COST: \`cargo nextest list\` always builds the workspace,
                        so a cold tree triggers a multi-minute cargo build.
                        OFF by default — the default run is cheap (stat-only).
  --json                Emit a machine-readable JSON object instead of text.
  -h, --help            Show this help and exit.

DEFAULT RUN (cheap, stat-only)
  Reports the target-dir on-disk size and the raw count of executables in
  <target>/debug/deps. The "stale" figure is only computed with --with-binaries
  (it needs the live nextest binary set to subtract); without it the raw dep
  executable count is reported as a proxy.

JSON SHAPE
  {
    "targetDir": string,
    "targetExists": boolean,
    "targetSizeBytes": number,
    "targetSizeHuman": string,
    "depExecutables": number,
    "currentTestBinaries": number | null,
    "currentTestBinariesSizeBytes": number | null,
    "currentTestBinariesSizeHuman": string | null,
    "staleExecutables": number | null
  }

EXIT
  0  success (including a missing target dir in default mode).
  2  hard error (bad arguments, or an explicitly requested --target-dir that
     does not exist, or nextest failure under --with-binaries).
`;

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

// Human-readable byte size. 1536 -> "1.5 KB", 0 -> "0 B".
function humanSize(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  // Integer bytes print without a decimal; larger units keep one decimal.
  const text = unit === 0 ? String(Math.round(value)) : value.toFixed(1);
  return `${text} ${units[unit]}`;
}

// ---------------------------------------------------------------------------
// Directory size walk (stat-only, no file-content reads)
// ---------------------------------------------------------------------------

// Recursively sum file sizes under `dir`. Uses an explicit stack to avoid deep
// call recursion on a very large tree. Does NOT follow symlinks (stat the link
// target only for regular files surfaced as such by readdirSync dirents).
function dirSize(dir) {
  let total = 0;
  let fileCount = 0;
  const stack = [dir];
  while (stack.length > 0) {
    const current = stack.pop();
    let entries;
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      // Unreadable / vanished mid-walk — skip.
      continue;
    }
    for (const entry of entries) {
      const full = join(current, entry.name);
      if (entry.isDirectory()) {
        // Skip symlinked directories to avoid cycles / double counting.
        if (entry.isSymbolicLink && entry.isSymbolicLink()) continue;
        stack.push(full);
      } else if (entry.isFile()) {
        try {
          total += statSync(full).size;
          fileCount++;
        } catch {
          // File vanished mid-walk — skip.
        }
      }
      // Symlinks-to-files and other node types are not counted.
    }
  }
  return { bytes: total, fileCount };
}

// ---------------------------------------------------------------------------
// Dep-executable enumeration
// ---------------------------------------------------------------------------

// A cargo test/bin artifact in <target>/debug/deps is, on Unix, a regular file
// with the executable bit and no extension (this naturally excludes `.d`, `.o`,
// `.rlib`, `.rmeta`, and `.dylib`/`.so` shared libs, which either lack the exec
// bit or carry an extension). On Windows it is a `*.exe`. We additionally
// require the cargo `name-<hex16>` suffix to avoid counting stray files.
const HEX16_SUFFIX = /-[0-9a-f]{16}$/;

function isDepExecutable(dirent, fullPath) {
  if (!dirent.isFile()) return false;
  const name = dirent.name;
  if (IS_WIN) {
    if (extname(name).toLowerCase() !== ".exe") return false;
    const stem = name.slice(0, -".exe".length);
    return HEX16_SUFFIX.test(stem);
  }
  // Unix: no extension + executable bit + name-<hex16>.
  if (extname(name) !== "") return false;
  if (!HEX16_SUFFIX.test(name)) return false;
  let st;
  try {
    st = statSync(fullPath);
  } catch {
    return false;
  }
  // Any executable bit (owner/group/other).
  return (st.mode & 0o111) !== 0;
}

// Returns a Map<basename, sizeBytes> of dep executables in <target>/debug/deps.
function enumerateDepExecutables(targetDir) {
  const depsDir = join(targetDir, "debug", "deps");
  const result = new Map();
  let entries;
  try {
    entries = readdirSync(depsDir, { withFileTypes: true });
  } catch {
    return result; // deps dir absent — no executables.
  }
  for (const entry of entries) {
    const full = join(depsDir, entry.name);
    if (isDepExecutable(entry, full)) {
      let size = 0;
      try {
        size = statSync(full).size;
      } catch {
        size = 0;
      }
      result.set(entry.name, size);
    }
  }
  return result;
}

// ---------------------------------------------------------------------------
// nextest current-binary set (opt-in; triggers a build)
// ---------------------------------------------------------------------------

// Runs `cargo nextest list --workspace --list-type binaries-only --message-format json`
// and returns the set of CURRENT test-binary absolute paths. binaries-only avoids
// enumerating individual tests (lighter than the full listing) while still
// building the workspace. Parses the stable TestListSummary schema:
//   { "rust-suites": { <binary-id>: { "binary-path": <abs path>, ... }, ... } }
function listCurrentTestBinaries() {
  const args = [
    "nextest",
    "list",
    "--workspace",
    "--list-type",
    "binaries-only",
    "--message-format",
    "json",
  ];
  const run = spawnSync("cargo", args, {
    cwd: REPO_ROOT,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  if (run.error) {
    fail(`failed to spawn \`cargo nextest list\`: ${run.error.message}`);
  }
  if (run.status !== 0) {
    const stderrTail = (run.stderr || "").split("\n").slice(-15).join("\n");
    fail(`\`cargo nextest list\` exited with status ${run.status}.\n${stderrTail}`);
  }
  // nextest emits build/cargo chatter on stderr and the JSON document on stdout.
  // Parse the last JSON object on stdout to be robust to any leading noise.
  const stdout = run.stdout || "";
  const summary = parseLastJsonObject(stdout);
  if (summary == null) {
    fail(`could not parse JSON from \`cargo nextest list\` stdout`);
  }
  const suites = summary["rust-suites"] || {};
  const paths = new Set();
  for (const key of Object.keys(suites)) {
    const suite = suites[key];
    const binPath = suite && suite["binary-path"];
    if (typeof binPath === "string" && binPath.length > 0) {
      paths.add(binPath);
    }
  }
  return paths;
}

// Extract the last top-level JSON object from a string. nextest writes a single
// JSON document to stdout, but parse defensively in case of stray prefix output.
function parseLastJsonObject(text) {
  const trimmed = text.trim();
  if (trimmed.length === 0) return null;
  // Fast path: the whole stdout is the JSON document.
  try {
    return JSON.parse(trimmed);
  } catch {
    // Fall through to a brace-scan for the last balanced object.
  }
  let depth = 0;
  let start = -1;
  let lastObject = null;
  let inString = false;
  let escaped = false;
  for (let i = 0; i < trimmed.length; i++) {
    const ch = trimmed[i];
    if (inString) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') inString = false;
      continue;
    }
    if (ch === '"') {
      inString = true;
    } else if (ch === "{") {
      if (depth === 0) start = i;
      depth++;
    } else if (ch === "}") {
      depth--;
      if (depth === 0 && start >= 0) {
        const candidate = trimmed.slice(start, i + 1);
        try {
          lastObject = JSON.parse(candidate);
        } catch {
          // ignore non-JSON balanced span
        }
        start = -1;
      }
    }
  }
  return lastObject;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) {
    process.stdout.write(HELP);
    process.exit(0);
  }

  const explicitTarget = opts.targetDir != null;
  const targetDir = resolve(
    opts.targetDir || process.env.CARGO_TARGET_DIR || join(REPO_ROOT, "target"),
  );

  // Does the target dir exist?
  let targetExists = false;
  try {
    targetExists = statSync(targetDir).isDirectory();
  } catch {
    targetExists = false;
  }

  if (!targetExists) {
    if (explicitTarget) {
      // Explicitly requested but missing -> hard error.
      fail(`target dir does not exist: ${targetDir}`);
    }
    // Default-mode missing target -> report zeros, exit 0.
    const report = {
      targetDir,
      targetExists: false,
      targetSizeBytes: 0,
      targetSizeHuman: humanSize(0),
      depExecutables: 0,
      currentTestBinaries: null,
      currentTestBinariesSizeBytes: null,
      currentTestBinariesSizeHuman: null,
      staleExecutables: null,
    };
    emit(report, opts, { fileCount: 0 });
    process.exit(0);
  }

  // 1. Target dir size.
  const { bytes: targetSizeBytes, fileCount } = dirSize(targetDir);

  // 3a. Dep executables (always cheap).
  const depExecs = enumerateDepExecutables(targetDir);

  let currentTestBinaries = null;
  let currentTestBinariesSizeBytes = null;
  let staleExecutables = null;

  if (opts.withBinaries) {
    // 2 + 3b. Current nextest binary set (triggers a build).
    const currentPaths = listCurrentTestBinaries();
    currentTestBinaries = currentPaths.size;

    // Sum on-disk size of the current binaries.
    let sum = 0;
    const currentBasenames = new Set();
    for (const p of currentPaths) {
      const base = p.split(/[\\/]/).pop();
      if (base) currentBasenames.add(base);
      try {
        sum += statSync(p).size;
      } catch {
        // binary path reported but unreadable — count it, size 0.
      }
    }
    currentTestBinariesSizeBytes = sum;

    // Stale = dep executables NOT in the current binary set.
    // Match by basename (nextest paths point at <target>/debug/deps/<name-hash>).
    let stale = 0;
    for (const base of depExecs.keys()) {
      if (!currentBasenames.has(base)) stale++;
    }
    staleExecutables = stale;
  }

  const report = {
    targetDir,
    targetExists: true,
    targetSizeBytes,
    targetSizeHuman: humanSize(targetSizeBytes),
    depExecutables: depExecs.size,
    currentTestBinaries,
    currentTestBinariesSizeBytes,
    currentTestBinariesSizeHuman:
      currentTestBinariesSizeBytes == null ? null : humanSize(currentTestBinariesSizeBytes),
    staleExecutables,
  };

  emit(report, opts, { fileCount });
  process.exit(0);
}

function emit(report, opts, extra) {
  if (opts.json) {
    process.stdout.write(JSON.stringify(report, null, 2) + "\n");
    return;
  }
  const lines = [];
  lines.push(`Rust target health`);
  lines.push(`  target dir   : ${report.targetDir}`);
  if (!report.targetExists) {
    lines.push(`  status       : MISSING (reporting zeros)`);
    process.stdout.write(lines.join("\n") + "\n");
    return;
  }
  lines.push(
    `  target size  : ${report.targetSizeHuman} (${report.targetSizeBytes.toLocaleString()} bytes` +
      (extra && Number.isFinite(extra.fileCount)
        ? `, ${extra.fileCount.toLocaleString()} files)`
        : `)`),
  );
  lines.push(`  dep execs    : ${report.depExecutables} (in debug/deps)`);
  if (report.currentTestBinaries != null) {
    lines.push(
      `  test binaries: ${report.currentTestBinaries} current ` +
        `(${report.currentTestBinariesSizeHuman}, ` +
        `${report.currentTestBinariesSizeBytes.toLocaleString()} bytes)`,
    );
    lines.push(`  stale execs  : ${report.staleExecutables} (dep execs not in current set)`);
  } else {
    lines.push(
      `  stale execs  : n/a — re-run with --with-binaries to compute ` +
        `(dep-exec count above is a proxy)`,
    );
  }
  process.stdout.write(lines.join("\n") + "\n");
}

// Optional discriminating self-check, gated behind an env flag so it never runs
// in normal use. Asserts the human formatter on a known value.
if (process.env.TARGET_HEALTH_SELFTEST === "1") {
  const cases = [
    [0, "0 B"],
    [512, "512 B"],
    [1536, "1.5 KB"],
    [1024 * 1024, "1.0 MB"],
    [Math.round(1.5 * 1024 * 1024 * 1024), "1.5 GB"],
  ];
  let ok = true;
  for (const [input, want] of cases) {
    const got = humanSize(input);
    if (got !== want) {
      console.error(`selftest FAIL: humanSize(${input}) = ${got}, want ${want}`);
      ok = false;
    }
  }
  // Path separator sanity: join must use the platform separator.
  if (join("a", "b") !== `a${sep}b`) {
    console.error(`selftest FAIL: join separator mismatch`);
    ok = false;
  }
  if (!ok) process.exit(1);
  console.log("selftest OK");
  process.exit(0);
}

main();
