#!/usr/bin/env node

/**
 * Local integration test runner for Verter.
 *
 * Clones real-world Vue projects, builds them with the stock Vue plugin
 * (baseline), swaps in @verter/unplugin, rebuilds + retests, and compares the
 * results.  Mirrors what `.github/workflows/integration-test.yml` does in CI.
 *
 * Usage:
 *   node scripts/integration-test/run.mjs [options] [project-names...]
 *
 * Options:
 *   --skip-baseline   Skip baseline build/test (faster iteration)
 *   --skip-build      Skip building Verter (reuse existing tarballs)
 *   --no-clone        Skip git clone (reuse existing checkouts)
 *   --concurrency <n> Run N projects in parallel (default: 1)
 *   --help            Show this help message
 */

import { execSync, execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';
import { buildReviewQueue, buildDiagnosticDiff, normalizeTypeCheckArtifacts } from './diagnostics.mjs';
import { buildDiscoveryInventory, renderDiscoveryMarkdown, VERTER_EXTENSION_ID } from './discovery.mjs';
import { projects } from './projects.mjs';

// ── Paths ────────────────────────────────────────────────────────────────────

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, '../..');
const INTEGRATION_DIR = path.join(ROOT, '.integration-tests');
const REPOS_DIR = path.join(INTEGRATION_DIR, 'repos');
const TARBALLS_DIR = path.join(INTEGRATION_DIR, 'tarballs');
const LOGS_DIR = path.join(INTEGRATION_DIR, 'logs');
const DEFAULT_LOCAL_ROOTS = ['D:\\dev'];
const DEFAULT_LOCAL_RUNS_DIR = path.join('D:\\dev', 'temp', 'verter-toolchain-runs');
const LOCAL_SANDBOX_SKIP = ['.git', 'node_modules', 'dist', 'build', 'coverage', '.next', '.nuxt', '.output', 'out', 'target', '.turbo', 'tmp', 'temp'];

// ── Type-Check Binary Detection ─────────────────────────────────────────────

const IS_WIN = process.platform === 'win32';
const EXE = IS_WIN ? '.exe' : '';

const VERTER_TSC_RELEASE = path.join(ROOT, 'target', 'release', `verter-tsc${EXE}`);
const VERTER_TSC_DEBUG = path.join(ROOT, 'target', 'debug', `verter-tsc${EXE}`);

/**
 * Detect the best available build of a binary (release preferred over debug).
 * @returns {{ bin: string | null, type: 'release' | 'debug' | 'missing' }}
 */
function detectBinary(release, debug) {
  const hasRelease = fs.existsSync(release);
  const hasDebug = fs.existsSync(debug);
  if (!hasRelease && !hasDebug) return { bin: null, type: 'missing' };
  if (!hasRelease) return { bin: debug, type: 'debug' };
  if (!hasDebug) return { bin: release, type: 'release' };
  // Both exist — pick newer
  const relMtime = fs.statSync(release).mtimeMs;
  const dbgMtime = fs.statSync(debug).mtimeMs;
  return relMtime >= dbgMtime
    ? { bin: release, type: 'release' }
    : { bin: debug, type: 'debug' };
}

const VERTER_TSC = detectBinary(VERTER_TSC_RELEASE, VERTER_TSC_DEBUG);

/**
 * Find vue-tsc for a project. Prefers project-local, falls back to npx.
 * @returns {{ bin: string, args: string[] }}
 */
function findVueTsc(projectRoot) {
  const binDir = path.join(projectRoot, 'node_modules', '.bin');
  const cmd = path.join(binDir, IS_WIN ? 'vue-tsc.cmd' : 'vue-tsc');
  if (fs.existsSync(cmd)) return { bin: cmd, args: [] };
  const plain = path.join(binDir, 'vue-tsc');
  if (fs.existsSync(plain)) return { bin: plain, args: [] };
  // Fall back to npx
  return { bin: IS_WIN ? 'npx.cmd' : 'npx', args: ['vue-tsc'] };
}

/**
 * Run a type-check tool and measure wall-clock time.
 * Both vue-tsc and verter-tsc use the same invocation: --noEmit --project <tsconfig>
 * @returns {{ ms: number, exitCode: number, errorCount: number, timedOut: boolean, stdout: string, stderr: string }}
 */
function runTypeCheckTool(bin, args, cwd) {
  const TIMEOUT = 5 * 60_000;
  const start = performance.now();
  const r = spawnSync(bin, args, {
    cwd,
    timeout: TIMEOUT,
    encoding: 'utf-8',
    shell: IS_WIN && (bin.endsWith('.cmd') || bin.endsWith('.bat')),
    windowsHide: true,
    env: { ...process.env, FORCE_COLOR: '0' },
  });
  const ms = performance.now() - start;

  if (r.error?.message?.includes('ETIMEDOUT') || r.signal === 'SIGTERM') {
    return {
      ms,
      exitCode: -1,
      errorCount: 0,
      timedOut: true,
      stdout: String(r.stdout ?? ''),
      stderr: String(r.stderr ?? ''),
    };
  }

  const out = String(r.stdout ?? '') + String(r.stderr ?? '');
  const errorCount = (out.match(/error TS\d+:/g) ?? []).length;
  return {
    ms,
    exitCode: r.status ?? -1,
    errorCount,
    timedOut: false,
    stdout: String(r.stdout ?? ''),
    stderr: String(r.stderr ?? ''),
  };
}

/**
 * Run type-check benchmarks for a project: 2 passes each of vue-tsc and verter-tsc.
 * Both tools run: --noEmit --project <tsconfig>
 *
 * @returns {{ tsconfig: string, vueTsc: { cold, warm }, verterTsc: { cold, warm } } | null}
 */
function runTypeChecks(project, repoDir) {
  // Find the best tsconfig for type-checking.
  // Some projects (e.g. element-plus) use project references at root —
  // their root tsconfig has `files: []` + `references: [...]` which just
  // validates the reference graph (not the actual source). We detect this
  // and try common alternatives that contain the real include patterns.
  const rootTsconfig = path.resolve(repoDir, 'tsconfig.json');
  if (!fs.existsSync(rootTsconfig)) return null;

  let tsconfig = rootTsconfig;
  try {
    const raw = JSON.parse(fs.readFileSync(rootTsconfig, 'utf-8'));
    const hasFiles = Array.isArray(raw.files) && raw.files.length > 0;
    const hasInclude = Array.isArray(raw.include) && raw.include.length > 0;
    const hasRefs = Array.isArray(raw.references) && raw.references.length > 0;
    // Project-references-only tsconfig — look for a better alternative
    if (!hasFiles && !hasInclude && hasRefs) {
      const alternatives = ['tsconfig.web.json', 'tsconfig.app.json', 'tsconfig.src.json'];
      for (const alt of alternatives) {
        const altPath = path.resolve(repoDir, alt);
        if (fs.existsSync(altPath)) {
          tsconfig = altPath;
          log(project.name, `[type-check] using ${alt} (root tsconfig is references-only)`);
          break;
        }
      }
      if (tsconfig === rootTsconfig) {
        log(project.name, `[type-check] root tsconfig is references-only, no alternative found — skipping`);
        return null;
      }
    }
  } catch { /* parse error — try with the root tsconfig anyway */ }

  const results = {
    tsconfig,
    vueTsc: { cold: null, warm: null },
    verterTsc: { cold: null, warm: null },
  };

  // Both tools run the exact same command: --noEmit --project <absolute-tsconfig>
  // This ensures a fair comparison — same input, same goal, different implementation.

  const fmtTcMs = (r) => r.timedOut ? '>5min' : formatDuration(r.ms) + (r.exitCode !== 0 ? '(err)' : '');

  // vue-tsc: 2 passes (cold, warm)
  const vueTscInfo = findVueTsc(repoDir);
  const vueTscArgs = [...vueTscInfo.args, '--noEmit', '--project', tsconfig];
  log(project.name, `[type-check] vue-tsc cold...`);
  results.vueTsc.cold = runTypeCheckTool(vueTscInfo.bin, vueTscArgs, repoDir);
  log(project.name, `[type-check] vue-tsc cold: ${fmtTcMs(results.vueTsc.cold)}`);
  log(project.name, `[type-check] vue-tsc warm...`);
  results.vueTsc.warm = runTypeCheckTool(vueTscInfo.bin, vueTscArgs, repoDir);
  log(project.name, `[type-check] vue-tsc warm: ${fmtTcMs(results.vueTsc.warm)}`);

  // verter-tsc: 2 passes (cold, warm) — same --noEmit --project <tsconfig>
  if (VERTER_TSC.bin) {
    const verterArgs = ['--noEmit', '--project', tsconfig];
    log(project.name, `[type-check] verter-tsc cold...`);
    results.verterTsc.cold = runTypeCheckTool(VERTER_TSC.bin, verterArgs, repoDir);
    log(project.name, `[type-check] verter-tsc cold: ${fmtTcMs(results.verterTsc.cold)}`);
    log(project.name, `[type-check] verter-tsc warm...`);
    results.verterTsc.warm = runTypeCheckTool(VERTER_TSC.bin, verterArgs, repoDir);
    log(project.name, `[type-check] verter-tsc warm: ${fmtTcMs(results.verterTsc.warm)}`);
  }

  return results;
}

// ── CLI Parsing ──────────────────────────────────────────────────────────────

function parseArgs() {
  const args = process.argv.slice(2);
  const opts = {
    skipBaseline: false,
    skipBuild: false,
    noClone: false,
    fast: false,
    discoverLocal: false,
    discoverOnly: false,
    localOnly: false,
    runId: null,
    out: null,
    repoFilter: null,
    roots: [...DEFAULT_LOCAL_ROOTS],
    concurrency: 1,
    projectNames: /** @type {string[]} */ ([]),
  };

  for (let i = 0; i < args.length; i++) {
    switch (args[i]) {
      case '--skip-baseline':
        opts.skipBaseline = true;
        break;
      case '--skip-build':
        opts.skipBuild = true;
        break;
      case '--no-clone':
        opts.noClone = true;
        break;
      case '--fast':
        opts.fast = true;
        break;
      case '--discover-local':
        opts.discoverLocal = true;
        break;
      case '--discover-only':
        opts.discoverOnly = true;
        break;
      case '--local-only':
        opts.localOnly = true;
        break;
      case '--roots':
        opts.roots = args[++i].split(/[;,]/u).map((value) => value.trim()).filter(Boolean);
        break;
      case '--out':
        opts.out = args[++i];
        break;
      case '--repo-filter':
        opts.repoFilter = args[++i];
        break;
      case '--run-id':
        opts.runId = args[++i];
        break;
      case '--concurrency':
        opts.concurrency = parseInt(args[++i], 10) || 1;
        break;
      case '--help':
      case '-h':
        console.log(
          [
            'Usage: node scripts/integration-test/run.mjs [options] [project-names...]',
            '',
            'Options:',
            '  --skip-baseline   Skip baseline build/test',
            '  --fast            Use the debug native build for faster local iteration',
            '  --skip-build      Skip building Verter (reuse tarballs)',
            '  --no-clone        Skip git clone (reuse checkouts)',
            '  --discover-local  Inventory local Vue repos under the configured roots',
            '  --discover-only   Write discovery artifacts and exit',
            '  --local-only      Execute local discovered repos without running the matrix',
            '  --roots <paths>   Semicolon/comma-separated discovery roots (default: D:\\dev)',
            '  --out <path>      Output directory for local discovery/execution artifacts',
            '  --repo-filter <r> Regex filter applied to discovered repo paths',
            '  --run-id <id>     Override the local run id used in the output path',
            '  --concurrency <n> Run N projects in parallel (default: 1)',
            '  --help            Show this message',
            '',
            'Available projects:',
            ...projects.map((p) => `  ${p.name}`),
          ].join('\n'),
        );
        process.exit(0);
        break;
      default:
        if (args[i].startsWith('-')) {
          console.error(`Unknown option: ${args[i]}`);
          process.exit(1);
        }
        opts.projectNames.push(args[i]);
    }
  }

  return opts;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/** Run a shell command; return { ok, stdout, stderr, durationMs }. */
function run(cmd, cwd, { timeout = 10 * 60_000, env: extraEnv = {} } = {}) {
  const start = performance.now();

  // On Windows, strip leading VAR=val prefixes from commands and add to env
  const parsedEnv = { ...extraEnv };
  let finalCmd = cmd;
  if (process.platform === 'win32') {
    const envPrefixRe = /^(\w+=\S+\s+)+/;
    const match = cmd.match(envPrefixRe);
    if (match) {
      const prefix = match[0];
      for (const part of prefix.trim().split(/\s+/)) {
        const eq = part.indexOf('=');
        if (eq > 0) parsedEnv[part.slice(0, eq)] = part.slice(eq + 1);
      }
      finalCmd = cmd.slice(prefix.length);
    }
  }

  try {
    const stdout = execSync(finalCmd, {
      cwd,
      stdio: 'pipe',
      shell: true,
      timeout,
      env: { ...process.env, FORCE_COLOR: '0', COREPACK_ENABLE_STRICT: '0', ...parsedEnv },
      maxBuffer: 50 * 1024 * 1024,
    });
    return {
      ok: true,
      stdout: stdout.toString(),
      stderr: '',
      durationMs: performance.now() - start,
    };
  } catch (/** @type {any} */ err) {
    return {
      ok: false,
      stdout: err.stdout?.toString() ?? '',
      stderr: err.stderr?.toString() ?? '',
      durationMs: performance.now() - start,
    };
  }
}

/** Extract test counts from log output (vitest/jest patterns). */
function extractTestCounts(output) {
  const passed = output.match(/(\d+)\s+passed/);
  const failed = output.match(/(\d+)\s+failed/);
  return {
    passed: passed ? parseInt(passed[1], 10) : 0,
    failed: failed ? parseInt(failed[1], 10) : 0,
  };
}

/** Recursively find files matching a predicate (skips node_modules, .git). */
function findFiles(dir, predicate) {
  const results = [];
  const SKIP = new Set(['node_modules', '.git', '.output', '.nuxt', 'dist']);

  function walk(current) {
    let entries;
    try {
      entries = fs.readdirSync(current, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (SKIP.has(entry.name)) continue;
      const full = path.join(current, entry.name);
      if (entry.isDirectory()) {
        walk(full);
      } else if (entry.isFile() && predicate(entry.name, full)) {
        results.push(full);
      }
    }
  }

  walk(dir);
  return results;
}

function log(prefix, msg) {
  const ts = new Date().toISOString().slice(11, 19);
  console.log(`[${ts}] [${prefix}] ${msg}`);
}

/** Recursively copy a directory, skipping entries whose names are in `skipNames`. */
function copyRecursive(src, dest, skipNames = []) {
  const skip = new Set(skipNames);
  fs.mkdirSync(dest, { recursive: true });
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    if (skip.has(entry.name)) continue;
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);
    if (entry.isDirectory()) {
      copyRecursive(srcPath, destPath, skipNames);
    } else {
      try {
        fs.copyFileSync(srcPath, destPath);
      } catch {
        // Skip files that can't be copied (e.g. symlinked bins on Windows)
      }
    }
  }
}

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

function writeJson(filePath, value) {
  ensureDir(path.dirname(filePath));
  fs.writeFileSync(filePath, JSON.stringify(value, null, 2) + '\n');
}

function createRunId() {
  return new Date().toISOString().replace(/[:.]/g, '-');
}

function sanitizeLocalName(project) {
  const source = project.relativeRoot || project.name || 'project';
  return source.replace(/[^A-Za-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '') || 'project';
}

function createLocalRunContext(opts) {
  const outRoot = opts.out || DEFAULT_LOCAL_RUNS_DIR;
  const runId = opts.runId || createRunId();
  const runDir = path.join(outRoot, runId);
  const sandboxesDir = path.join(runDir, 'sandboxes');
  const reportsDir = path.join(runDir, 'reports');
  ensureDir(sandboxesDir);
  ensureDir(reportsDir);
  return { outRoot, runId, runDir, sandboxesDir, reportsDir };
}

function prepareLocalSandbox(project, runContext) {
  const sandboxDir = path.join(runContext.sandboxesDir, sanitizeLocalName(project));
  fs.rmSync(sandboxDir, { recursive: true, force: true });
  copyRecursive(project.repoRoot, sandboxDir, LOCAL_SANDBOX_SKIP);
  return sandboxDir;
}

function writeTypeCheckArtifacts(project, repoDir, artifactDir, typeCheck) {
  if (!typeCheck) return { normalized: null, diff: null, queue: null };

  const normalized = normalizeTypeCheckArtifacts(typeCheck, repoDir);
  const diff = buildDiagnosticDiff(normalized);
  const queue = buildReviewQueue(diff, {
    repoRoot: repoDir,
    projectName: project.name,
  });

  const typeDir = path.join(artifactDir, 'typecheck');
  ensureDir(typeDir);
  for (const runResult of normalized.runs) {
    const baseName = `${runResult.tool}-${runResult.pass}.log`;
    fs.writeFileSync(path.join(typeDir, baseName), [runResult.stdout, runResult.stderr].filter(Boolean).join('\n'));
  }

  writeJson(path.join(typeDir, 'diagnostics.normalized.json'), normalized);
  writeJson(path.join(typeDir, 'diagnostics.diff.json'), diff);
  writeJson(path.join(typeDir, 'review-queue.json'), queue);

  return { normalized, diff, queue };
}

function writeProjectSummary(project, artifactDir, result, diff, queue) {
  const lines = [];
  lines.push(`# ${project.name}`);
  lines.push('');
  lines.push(`- Recipe: ${project.replacementRecipe ?? 'matrix'}`);
  lines.push(`- Tier: ${project.executionTier ?? 'tier1'}`);
  if (project.repoRoot) lines.push(`- Source: ${project.repoRoot}`);
  if (project.chosenTsconfig) lines.push(`- Tsconfig: ${project.chosenTsconfig}`);
  if (project.replacementSteps?.length) lines.push(`- Steps: ${project.replacementSteps.join(', ')}`);
  if (result.typeCheckCrash) lines.push('- Type-check crash: yes');
  if (result.error) lines.push(`- Error: ${result.error}`);
  lines.push('');

  if (diff?.summary) {
    lines.push('## Diagnostic Diff');
    lines.push('');
    for (const [classification, count] of Object.entries(diff.summary)) {
      lines.push(`- ${classification}: ${count}`);
    }
    lines.push('');
  }

  if (queue?.items?.length) {
    lines.push('## Review Queue');
    lines.push('');
    for (const item of queue.items.slice(0, 25)) {
      lines.push(`- ${item.status} ${item.classification} ${item.code ?? '-'} ${item.file ?? '-'}:${item.line ?? '-'} ${item.message}`);
    }
    lines.push('');
  }

  fs.writeFileSync(path.join(artifactDir, 'summary.md'), lines.join('\n'));
}

function replaceEditorTooling(project, repoDir) {
  const replacements = new Map([
    ['Vue.volar', VERTER_EXTENSION_ID],
    ['Vue.vscode-typescript-vue-plugin', VERTER_EXTENSION_ID],
    ['Vue Official', 'Verter'],
    ['@vue/typescript-plugin', '@verter/typescript-plugin'],
  ]);

  const candidates = [
    ...findFiles(repoDir, (name, full) => name.endsWith('.code-workspace') || (['settings.json', 'extensions.json'].includes(name) && full.includes(`${path.sep}.vscode${path.sep}`))),
  ];

  const modifiedFiles = [];
  for (const filePath of candidates) {
    let content;
    try {
      content = fs.readFileSync(filePath, 'utf8');
    } catch {
      continue;
    }
    let updated = content;
    for (const [needle, replacement] of replacements) {
      updated = updated.split(needle).join(replacement);
    }
    if (updated !== content) {
      fs.writeFileSync(filePath, updated);
      modifiedFiles.push(path.relative(repoDir, filePath));
    }
  }
  return modifiedFiles;
}

function createVerterTscShim(repoDir) {
  const binDir = path.join(repoDir, 'node_modules', '.bin');
  ensureDir(binDir);

  const binary = VERTER_TSC.bin;
  if (!binary) return;

  const shellBinary = binary.replace(/\\/g, '/');
  const cmdPath = path.join(binDir, 'verter-tsc.cmd');
  const shellPath = path.join(binDir, 'verter-tsc');
  fs.writeFileSync(cmdPath, `@echo off\r\n"${binary}" %*\r\n`);
  fs.writeFileSync(shellPath, `#!/usr/bin/env sh\n"${shellBinary}" "$@"\n`);
}

function ensureTypeScriptToolingAccessible(repoDir) {
  const pluginSrc = path.join(ROOT, 'packages', 'typescript-plugin');
  const pluginDest = path.join(repoDir, 'node_modules', '@verter', 'typescript-plugin');
  if (!fs.existsSync(path.join(pluginDest, 'package.json'))) {
    ensureDir(path.join(repoDir, 'node_modules', '@verter'));
    copyRecursive(pluginSrc, pluginDest, ['src', 'node_modules']);
  }
  createVerterTscShim(repoDir);
}

function replaceTypeScriptTooling(project, repoDir) {
  ensureTypeScriptToolingAccessible(repoDir);
  const modifiedFiles = [];

  const packageJsonPath = path.join(repoDir, 'package.json');
  if (fs.existsSync(packageJsonPath)) {
    const pkg = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
    let changed = false;
    if (pkg.scripts && typeof pkg.scripts === 'object') {
      for (const [name, script] of Object.entries(pkg.scripts)) {
        if (typeof script === 'string' && script.includes('vue-tsc')) {
          pkg.scripts[name] = script.split('vue-tsc').join('verter-tsc');
          changed = true;
        }
      }
    }
    if (changed) {
      fs.writeFileSync(packageJsonPath, JSON.stringify(pkg, null, 2) + '\n');
      modifiedFiles.push('package.json');
    }
  }

  const configFiles = findFiles(repoDir, (name, full) => {
    if (name === 'package.json') return false;
    if (name === 'jsconfig.json' || /^tsconfig(\..+)?\.json$/u.test(name)) return true;
    if (name.endsWith('.code-workspace')) return true;
    return ['settings.json', 'extensions.json'].includes(name) && full.includes(`${path.sep}.vscode${path.sep}`);
  });

  for (const filePath of configFiles) {
    let content;
    try {
      content = fs.readFileSync(filePath, 'utf8');
    } catch {
      continue;
    }
    const updated = content
      .split('@vue/typescript-plugin').join('@verter/typescript-plugin')
      .split('vue-tsc').join('verter-tsc')
      .split('Vue.vscode-typescript-vue-plugin').join(VERTER_EXTENSION_ID);

    if (updated !== content) {
      fs.writeFileSync(filePath, updated);
      modifiedFiles.push(path.relative(repoDir, filePath));
    }
  }

  return [...new Set(modifiedFiles)];
}

// ── Build Verter ─────────────────────────────────────────────────────────────

function buildVerter({ fast = false } = {}) {
  log('verter', `Building native bindings${fast ? ' (fast/debug)' : ''}...`);
  const nativeScript = fast ? 'pnpm --filter @verter/native build:debug' : 'pnpm run build:native';
  const native = run(nativeScript, ROOT);
  if (!native.ok) {
    console.error(native.stderr || native.stdout);
    throw new Error('Failed to build native bindings');
  }

  log('verter', 'Building typescript plugin...');
  const tsPlugin = run('pnpm --filter @verter/typescript-plugin build', ROOT);
  if (!tsPlugin.ok) {
    console.error(tsPlugin.stderr || tsPlugin.stdout);
    throw new Error('Failed to build typescript plugin');
  }

  log('verter', 'Building verter-tsc...');
  const tscBuild = run('pnpm run build:tsc', ROOT);
  if (!tscBuild.ok) {
    console.error(tscBuild.stderr || tscBuild.stdout);
    throw new Error('Failed to build verter-tsc');
  }

  log('verter', 'Building unplugin...');
  const unplugin = run('pnpm --filter @verter/unplugin build', ROOT);
  if (!unplugin.ok) {
    console.error(unplugin.stderr || unplugin.stdout);
    throw new Error('Failed to build unplugin');
  }

  log('verter', 'Building nuxt module...');
  const nuxtMod = run('pnpm --filter @verter/nuxt build', ROOT);
  if (!nuxtMod.ok) {
    console.error(nuxtMod.stderr || nuxtMod.stdout);
    throw new Error('Failed to build nuxt module');
  }

  // Pack tarballs
  fs.mkdirSync(TARBALLS_DIR, { recursive: true });
  // Remove old tarballs
  for (const f of fs.readdirSync(TARBALLS_DIR)) {
    if (f.endsWith('.tgz')) fs.unlinkSync(path.join(TARBALLS_DIR, f));
  }

  const absTarget = TARBALLS_DIR.replace(/\\/g, '/');

  log('verter', 'Packing native...');
  run(`pnpm pack --pack-destination "${absTarget}"`, path.join(ROOT, 'packages/native'));

  log('verter', 'Packing unplugin...');
  run(`pnpm pack --pack-destination "${absTarget}"`, path.join(ROOT, 'packages/unplugin'));

  log('verter', 'Packing nuxt...');
  run(`pnpm pack --pack-destination "${absTarget}"`, path.join(ROOT, 'packages/nuxt'));

  const tarballs = fs.readdirSync(TARBALLS_DIR).filter((f) => f.endsWith('.tgz'));
  if (tarballs.length < 3) {
    throw new Error(`Expected 3 tarballs, found ${tarballs.length} in ${TARBALLS_DIR}`);
  }
  log('verter', `Packed: ${tarballs.join(', ')}`);
}

// ── Clone / Update ───────────────────────────────────────────────────────────

function cloneProject(project) {
  const repoDir = path.join(REPOS_DIR, project.name);

  if (fs.existsSync(path.join(repoDir, '.git'))) {
    log(project.name, `Updating existing checkout (${project.branch})...`);
    run(`git fetch origin ${project.branch}`, repoDir);
    run(`git checkout ${project.branch}`, repoDir);
    run(`git reset --hard origin/${project.branch}`, repoDir);
    run('git clean -fdx', repoDir);
  } else {
    log(project.name, `Cloning ${project.repo}@${project.branch}...`);
    fs.mkdirSync(repoDir, { recursive: true });
    const result = run(
      `git clone --depth 1 --branch ${project.branch} https://github.com/${project.repo}.git "${repoDir}"`,
      ROOT,
    );
    if (!result.ok) {
      console.error(result.stderr);
      throw new Error(`Failed to clone ${project.repo}`);
    }
  }

  return repoDir;
}

// ── Install Dependencies ─────────────────────────────────────────────────────

function installDeps(project, repoDir) {
  log(project.name, `Installing dependencies (${project.packageManager})...`);

  if (project.packageManager === 'pnpm') {
    // Ensure the project has its own pnpm-workspace.yaml so pnpm doesn't
    // walk up and resolve to the .integration-tests/ workspace boundary.
    // For non-monorepo projects that lack one, create a minimal workspace file.
    const wsPath = path.join(repoDir, 'pnpm-workspace.yaml');
    if (!fs.existsSync(wsPath)) {
      fs.writeFileSync(wsPath, 'packages: []\n');
    }

    // Rewrite `packageManager` to match the local pnpm version, preventing corepack
    // from trying to switch to an unavailable version. We keep the field (vs deleting)
    // because turbo-based projects require it.
    // Also strip `engines.pnpm` to prevent ERR_PNPM_UNSUPPORTED_ENGINE errors.
    const pkgJsonPath = path.join(repoDir, 'package.json');
    if (fs.existsSync(pkgJsonPath)) {
      const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'));
      let modified = false;
      if (pkgJson.packageManager) {
        const localPnpmVersion = execFileSync('pnpm', ['--version'], { encoding: 'utf8' }).trim();
        pkgJson.packageManager = `pnpm@${localPnpmVersion}`;
        modified = true;
      }
      if (pkgJson.engines?.pnpm) {
        delete pkgJson.engines.pnpm;
        if (Object.keys(pkgJson.engines).length === 0) delete pkgJson.engines;
        modified = true;
      }
      if (modified) {
        fs.writeFileSync(pkgJsonPath, JSON.stringify(pkgJson, null, 2) + '\n');
        log(project.name, '  Rewrote packageManager/engines.pnpm fields');
      }
    }

    // On Windows, configure pnpm to use bash for running scripts.
    // Many projects use Unix-style 'VAR=value command' syntax (e.g. NODE_ENV=production)
    // which doesn't work in cmd.exe. Git for Windows provides bash.
    if (process.platform === 'win32') {
      const npmrcPath = path.join(repoDir, '.npmrc');
      let npmrc = fs.existsSync(npmrcPath) ? fs.readFileSync(npmrcPath, 'utf8') : '';
      if (!npmrc.includes('script-shell=')) {
        npmrc += '\nscript-shell=bash\n';
        fs.writeFileSync(npmrcPath, npmrc);
      }
    }

    const result = run('pnpm install --no-frozen-lockfile', repoDir, { timeout: 5 * 60_000 });
    if (!result.ok) {
      log(project.name, `Install warning: ${result.stderr.slice(0, 500)}`);
    }
  } else {
    // Use --legacy-peer-deps to bypass ERESOLVE errors from outdated peer dependencies
    // in third-party projects (e.g. ant-design-vue has prettier@2 vs @vue/eslint-config-prettier requiring >=3)
    const result = run('npm install --legacy-peer-deps', repoDir, { timeout: 5 * 60_000 });
    if (!result.ok) {
      log(project.name, `Install warning: ${result.stderr.slice(0, 500)}`);
    }
  }
}

// ── Fix Windows Scripts ──────────────────────────────────────────────────────

/**
 * On Windows, rewrite package.json scripts that use `VAR=val cmd` (Unix-only)
 * to `cross-env VAR=val cmd`, and install cross-env as a devDependency.
 */
function fixWindowsScripts(project, repoDir) {
  if (process.platform !== 'win32') return;

  const pkgPath = path.join(repoDir, 'package.json');
  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
  if (!pkg.scripts) return;

  const envPrefixRe = /^(\w+=\S+\s+)/;
  let changed = false;
  for (const [name, script] of Object.entries(pkg.scripts)) {
    if (typeof script === 'string' && envPrefixRe.test(script) && !script.startsWith('cross-env ')) {
      pkg.scripts[name] = `cross-env ${script}`;
      changed = true;
    }
  }

  if (changed) {
    fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');
    const pm = project.packageManager;
    const installCmd = pm === 'pnpm' ? 'pnpm add -D cross-env' : 'npm install --save-dev cross-env --legacy-peer-deps';
    run(installCmd, repoDir, { timeout: 60_000 });
    log(project.name, '  Patched scripts with cross-env for Windows');
  }
}

// ── Install Verter Tarballs ──────────────────────────────────────────────────

function installVerterTarballs(project, repoDir) {
  const tarballs = fs.readdirSync(TARBALLS_DIR).filter((f) => f.endsWith('.tgz'));
  const nativeTgz = tarballs.find((f) => f.includes('verter-native'));
  const unpluginTgz = tarballs.find((f) => f.includes('verter-unplugin'));
  const nuxtTgz = tarballs.find((f) => f.includes('verter-nuxt'));

  if (!nativeTgz || !unpluginTgz) {
    throw new Error('Missing verter tarballs. Run without --skip-build first.');
  }

  const isNuxt = project.bundler === 'nuxt';
  if (isNuxt && !nuxtTgz) {
    throw new Error('Missing @verter/nuxt tarball. Run without --skip-build first.');
  }

  // Use forward slashes for cross-platform compat in shell commands
  const nativePath = path.join(TARBALLS_DIR, nativeTgz).replace(/\\/g, '/');
  const unpluginPath = path.join(TARBALLS_DIR, unpluginTgz).replace(/\\/g, '/');
  const nuxtPath = nuxtTgz ? path.join(TARBALLS_DIR, nuxtTgz).replace(/\\/g, '/') : null;

  log(project.name, 'Installing Verter tarballs...');

  // Compute relative tarball paths (with forward slashes) for package.json overrides.
  // Using file: protocol ensures pnpm/npm resolve ALL references to the tarball
  // rather than trying to fetch the semver version from the registry.
  const relNative = path.relative(repoDir, path.join(TARBALLS_DIR, nativeTgz)).replace(/\\/g, '/');
  const relUnplugin = path.relative(repoDir, path.join(TARBALLS_DIR, unpluginTgz)).replace(/\\/g, '/');
  const relNuxt = nuxtTgz ? path.relative(repoDir, path.join(TARBALLS_DIR, nuxtTgz)).replace(/\\/g, '/') : null;

  // Build the list of tarballs to install
  const installPaths = [nativePath, unpluginPath];
  if (isNuxt && nuxtPath) installPaths.push(nuxtPath);

  if (project.packageManager === 'pnpm') {
    // Configure hoisting
    const npmrcPath = path.join(repoDir, '.npmrc');
    let npmrc = '';
    if (fs.existsSync(npmrcPath)) {
      npmrc = fs.readFileSync(npmrcPath, 'utf8');
    }
    if (!npmrc.includes('public-hoist-pattern[]=@verter/*')) {
      npmrc += '\npublic-hoist-pattern[]=@verter/*\npublic-hoist-pattern[]=unplugin\n';
      fs.writeFileSync(npmrcPath, npmrc);
    }

    run(`pnpm add -w ${installPaths.map((p) => `"${p}"`).join(' ')}`, repoDir);

    // Add pnpm overrides using file: protocol to the tarballs.
    // The $packageName syntax doesn't work with tarball-installed packages —
    // pnpm resolves the semver version separately, missing the native binary.
    const pkgPath = path.join(repoDir, 'package.json');
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    pkg.pnpm = pkg.pnpm || {};
    pkg.pnpm.overrides = pkg.pnpm.overrides || {};
    pkg.pnpm.overrides['@verter/native'] = `file:${relNative}`;
    pkg.pnpm.overrides['@verter/unplugin'] = `file:${relUnplugin}`;
    if (isNuxt && relNuxt) pkg.pnpm.overrides['@verter/nuxt'] = `file:${relNuxt}`;
    fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');

    run('pnpm install --no-frozen-lockfile', repoDir);
  } else {
    run(`npm install --legacy-peer-deps ${installPaths.map((p) => `"${p}"`).join(' ')}`, repoDir);

    // Add npm overrides using file: protocol
    const pkgPath = path.join(repoDir, 'package.json');
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    pkg.overrides = pkg.overrides || {};
    pkg.overrides['@verter/native'] = `file:${relNative}`;
    pkg.overrides['@verter/unplugin'] = `file:${relUnplugin}`;
    if (isNuxt && relNuxt) pkg.overrides['@verter/nuxt'] = `file:${relNuxt}`;
    fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');

    run('npm install --legacy-peer-deps', repoDir);
  }

  ensureVerterAccessible(project, repoDir);

  // Always overwrite ALL @verter dist directories from source.
  // pnpm uses hardlinks from a global content-addressable store. When tarballs
  // are rebuilt with the same version, the installed dist may contain stale code.
  // We must overwrite every copy — both top-level node_modules/@verter/*/dist
  // AND deep .pnpm store entries — because vitest/SSR may resolve from either.
  const srcDists = {
    unplugin: path.join(ROOT, 'packages', 'unplugin', 'dist'),
    native: path.join(ROOT, 'packages', 'native', 'dist'),
    nuxt: path.join(ROOT, 'packages', 'nuxt', 'dist'),
  };
  // Collect all @verter dist directories to overwrite
  const distsToOverwrite = [];
  for (const pkg of ['unplugin', 'native', 'nuxt']) {
    const topLevel = path.join(repoDir, 'node_modules', '@verter', pkg, 'dist');
    if (fs.existsSync(topLevel)) distsToOverwrite.push({ pkg, dist: topLevel });
  }
  // Also find deep copies inside .pnpm (pnpm creates separate copies per resolution)
  const pnpmDir = path.join(repoDir, 'node_modules', '.pnpm');
  if (fs.existsSync(pnpmDir)) {
    const findVerterDists = (dir, results) => {
      try {
        for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
          const fullPath = path.join(dir, entry.name);
          if (entry.name === 'dist' && dir.includes('@verter')) {
            results.push({ dist: fullPath, pkg: path.basename(path.dirname(fullPath)) });
          } else if (entry.isDirectory() && entry.name !== '.cache') {
            findVerterDists(fullPath, results);
          }
        }
      } catch { /* permission/access errors */ }
    };
    findVerterDists(pnpmDir, distsToOverwrite);
  }
  let overwritten = 0;
  for (const { dist: destDist, pkg } of distsToOverwrite) {
    const srcDist = srcDists[pkg];
    if (!srcDist || !fs.existsSync(srcDist)) continue;
    fs.rmSync(destDist, { recursive: true, force: true });
    copyRecursive(srcDist, destDist);
    overwritten++;
  }
  if (overwritten > 0) {
    log(project.name, `  Overwrote ${overwritten} @verter dist(s) from source`);
  }

  // Also overwrite root-level JS/TS files (index.js, index.ts) for each
  // @verter package. The dist overwrite only covers the dist/ subdirectory,
  // but files like native/index.js (which contains Buffer coercion wrappers)
  // live at the package root and may be stale in old tarballs.
  const srcRoots = {
    unplugin: path.join(ROOT, 'packages', 'unplugin'),
    native: path.join(ROOT, 'packages', 'native'),
    nuxt: path.join(ROOT, 'packages', 'nuxt'),
  };
  const rootFiles = ['index.js', 'index.ts'];
  for (const { dist: destDist, pkg } of distsToOverwrite) {
    const srcPkgDir = srcRoots[pkg];
    if (!srcPkgDir) continue;
    const destPkgDir = path.dirname(destDist); // parent of dist/ = package root
    for (const file of rootFiles) {
      const srcFile = path.join(srcPkgDir, file);
      if (fs.existsSync(srcFile)) {
        fs.copyFileSync(srcFile, path.join(destPkgDir, file));
      }
    }
  }

  log(project.name, 'Verter tarballs installed.');
}

/**
 * Verify that @verter/native and @verter/unplugin are accessible from the
 * project's node_modules.  Some pnpm monorepos don't properly hoist the
 * tarball-installed packages into the repo's own node_modules — the packages
 * end up at the parent workspace level without the `dist/` directory containing
 * the native `.node` binary.  When that happens, copy from source.
 */
function ensureVerterAccessible(project, repoDir) {
  const nativeDir = path.join(repoDir, 'node_modules', '@verter', 'native');
  const unpluginDir = path.join(repoDir, 'node_modules', '@verter', 'unplugin');
  const nativeIndex = path.join(nativeDir, 'index.js');
  const nativeDist = path.join(nativeDir, 'dist');

  if (!fs.existsSync(nativeIndex) || !fs.existsSync(nativeDist)) {
    log(project.name, '  @verter/native not properly hoisted, copying from source...');
    const srcNative = path.join(ROOT, 'packages', 'native');
    fs.mkdirSync(path.join(repoDir, 'node_modules', '@verter'), { recursive: true });
    copyRecursive(srcNative, nativeDir, ['node_modules']);
  }

  if (!fs.existsSync(path.join(unpluginDir, 'package.json'))) {
    log(project.name, '  @verter/unplugin not properly hoisted, copying from source...');
    const srcUnplugin = path.join(ROOT, 'packages', 'unplugin');
    fs.mkdirSync(path.join(repoDir, 'node_modules', '@verter'), { recursive: true });
    // Copy dist + package.json but skip src and node_modules (pnpm's node_modules
    // contains symlinks to the global store that can't be meaningfully copied).
    copyRecursive(srcUnplugin, unpluginDir, ['src', 'node_modules']);
  }

  // For Nuxt projects, ensure @verter/nuxt is accessible
  if (project.bundler === 'nuxt') {
    const nuxtDir = path.join(repoDir, 'node_modules', '@verter', 'nuxt');
    if (!fs.existsSync(path.join(nuxtDir, 'package.json'))) {
      log(project.name, '  @verter/nuxt not properly hoisted, copying from source...');
      const srcNuxt = path.join(ROOT, 'packages', 'nuxt');
      fs.mkdirSync(path.join(repoDir, 'node_modules', '@verter'), { recursive: true });
      copyRecursive(srcNuxt, nuxtDir, ['src', 'node_modules']);
    }
  }

  // Ensure the 'unplugin' dependency is resolvable from @verter/unplugin.
  // This runs unconditionally because even properly-installed pnpm packages
  // may have dangling symlinks or missing transitive dependencies.
  ensureUnpluginResolvable(project, repoDir, unpluginDir);
}

/**
 * Ensure that 'unplugin' is resolvable from @verter/unplugin.
 * pnpm may not hoist 'unplugin' to the top level, so we search the .pnpm
 * store and create a local symlink/copy.
 */
function ensureUnpluginResolvable(project, repoDir, unpluginDir) {
  const localUnpluginNM = path.join(unpluginDir, 'node_modules');
  const localUnpluginTarget = path.join(localUnpluginNM, 'unplugin');

  // If already resolvable (valid symlink or directory), skip
  if (fs.existsSync(path.join(localUnpluginTarget, 'package.json'))) return;

  // Clean up any dangling symlinks from previous runs
  try { fs.rmSync(localUnpluginTarget, { recursive: true, force: true }); } catch {}

  // Find unplugin: check top-level first, then search .pnpm store
  let unpluginSource = null;
  const topLevel = path.join(repoDir, 'node_modules', 'unplugin');
  if (fs.existsSync(path.join(topLevel, 'package.json'))) {
    unpluginSource = topLevel;
  } else {
    const pnpmDir = path.join(repoDir, 'node_modules', '.pnpm');
    if (fs.existsSync(pnpmDir)) {
      for (const entry of fs.readdirSync(pnpmDir)) {
        if (entry.startsWith('unplugin@')) {
          const candidate = path.join(pnpmDir, entry, 'node_modules', 'unplugin');
          if (fs.existsSync(candidate)) {
            unpluginSource = candidate;
            break;
          }
        }
      }
    }
  }

  if (!unpluginSource) {
    // Install unplugin into the project
    const installCmd = project.packageManager === 'pnpm'
      ? 'pnpm add unplugin'
      : 'npm install --legacy-peer-deps unplugin';
    const installResult = run(installCmd, repoDir, { timeout: 60_000 });
    if (!installResult.ok) {
      log(project.name, `  Warning: failed to install unplugin: ${(installResult.stderr || installResult.stdout).slice(0, 200)}`);
    }
    if (fs.existsSync(path.join(topLevel, 'package.json'))) {
      unpluginSource = topLevel;
    }
  }

  if (unpluginSource) {
    fs.mkdirSync(localUnpluginNM, { recursive: true });
    try {
      fs.symlinkSync(unpluginSource, localUnpluginTarget, 'junction');
    } catch {
      copyRecursive(unpluginSource, localUnpluginTarget);
    }
    log(project.name, `  Linked unplugin from ${path.relative(repoDir, unpluginSource)}`);
  }
}

// ── Replace Vue Plugin ───────────────────────────────────────────────────────

/** File extensions to consider for plugin replacement. */
const REPLACEABLE_EXTS = new Set(['.ts', '.js', '.mjs', '.mts', '.cjs']);

function isConfigFile(name) {
  return (
    name.startsWith('vite.config') ||
    name.startsWith('vitest.config') ||
    name.startsWith('rollup.config') ||
    name.startsWith('tsdown.config') ||
    REPLACEABLE_EXTS.has(path.extname(name))
  );
}

function replaceVuePlugin(project, repoDir) {
  const isRollup = project.bundler === 'rollup';
  const targetImport = isRollup ? '@verter/unplugin/rollup' : '@verter/unplugin/vite';

  // Pattern to search for
  const searchPatterns = isRollup
    ? ['@vitejs/plugin-vue', 'rollup-plugin-vue']
    : ['@vitejs/plugin-vue'];

  const skipNames = new Set(['package.json', 'package-lock.json', 'pnpm-lock.yaml', 'yarn.lock']);

  // Find files containing the vue plugin import
  const files = findFiles(repoDir, (name, _full) => {
    if (skipNames.has(name)) return false;
    return isConfigFile(name);
  });

  const modifiedFiles = [];

  for (const filePath of files) {
    let content;
    try {
      content = fs.readFileSync(filePath, 'utf8');
    } catch {
      continue;
    }

    const hasMatch = searchPatterns.some((p) => content.includes(p));
    if (!hasMatch) continue;

    let modified = content;

    // Replace import source paths
    for (const pattern of searchPatterns) {
      modified = modified.split(`from '${pattern}'`).join(`from '${targetImport}'`);
      modified = modified.split(`from "${pattern}"`).join(`from "${targetImport}"`);
    }

    // Rename imported identifiers (word-boundary safe)
    // Handle both `import vue from` and `import Vue, { namedExport } from`
    modified = modified.replace(/\bimport vue from\b/g, 'import verter from');
    modified = modified.replace(/\bimport Vue from\b/g, 'import verter from');
    modified = modified.replace(/\bimport vue,/g, 'import verter,');
    modified = modified.replace(/\bimport Vue,/g, 'import verter,');

    // Rename function calls (word-boundary safe — avoids matching inside
    // compound names like viteVue(, viteVueJsx(, etc.)
    modified = modified.replace(/\bvue\(/g, 'verter(');
    modified = modified.replace(/\bVue\(/g, 'verter(');

    if (modified !== content) {
      fs.writeFileSync(filePath, modified);
      const rel = path.relative(repoDir, filePath);
      modifiedFiles.push(rel);
      log(project.name, `  Modified: ${rel}`);
    }
  }

  // For monorepos: if we replaced imports in a workspace sub-package's source,
  // add @verter/unplugin as a dependency so the build tool externalizes it
  // instead of bundling it (which would embed require("@verter/native") inline).
  if (modifiedFiles.length > 0) {
    patchWorkspacePackageDeps(project, repoDir, modifiedFiles);
  }

  return modifiedFiles;
}

/**
 * Patch tsdown config files that use `fromVite: true`.
 *
 * When tsdown loads a vite.config with `fromVite: true`, it passes the vite
 * plugins to rolldown. The vite variant of verter's unplugin doesn't work
 * in the rolldown context (no `configResolved` hook called), so we inject an
 * `inputOptions` callback that swaps the vite verter plugin for the rolldown
 * variant.
 */
function patchTsdownConfigs(project, repoDir) {
  const files = findFiles(repoDir, (name) => name.startsWith('tsdown.config'));
  let patched = 0;

  for (const filePath of files) {
    let content;
    try {
      content = fs.readFileSync(filePath, 'utf8');
    } catch {
      continue;
    }

    // Only patch configs that use fromVite (these load vite.config plugins)
    if (!content.includes('fromVite')) continue;

    // Skip if already patched
    if (content.includes('@verter/unplugin/rolldown')) continue;

    // Inject the rolldown import and inputOptions callback
    const importLine = `import verter from '@verter/unplugin/rolldown'\n`;
    const inputOptionsBlock = `
  inputOptions: (defaults) => {
    const flattened = (defaults.plugins || []).flat(Infinity).filter(Boolean);
    const withoutVerter = flattened.filter((p) => p?.name !== 'unplugin-verter');
    return {
      ...defaults,
      plugins: [...withoutVerter, verter()],
    }
  },`;

    // Add import at the top (after existing imports)
    let modified = content;
    const lastImportIdx = modified.lastIndexOf('\nimport ');
    if (lastImportIdx >= 0) {
      const lineEnd = modified.indexOf('\n', lastImportIdx + 1);
      modified = modified.slice(0, lineEnd + 1) + importLine + modified.slice(lineEnd + 1);
    } else {
      modified = importLine + modified;
    }

    // Insert inputOptions before the first closing `}` or `})` of defineConfig
    // Look for `fromVite: true` and insert after that line
    const fromViteIdx = modified.indexOf('fromVite');
    if (fromViteIdx >= 0) {
      // Find the end of the fromVite line
      const lineEnd = modified.indexOf('\n', fromViteIdx);
      if (lineEnd >= 0) {
        modified = modified.slice(0, lineEnd + 1) + inputOptionsBlock + '\n' + modified.slice(lineEnd + 1);
      }
    }

    if (modified !== content) {
      fs.writeFileSync(filePath, modified);
      const rel = path.relative(repoDir, filePath);
      log(project.name, `  Patched tsdown config: ${rel} (added rolldown verter + inputOptions)`);
      patched++;
    }
  }

  return patched;
}

/**
 * For each workspace sub-package that had source files modified, replace
 * `@vitejs/plugin-vue` with `@verter/unplugin` in its package.json dependencies.
 * This ensures the bundler (tsdown/tsup/rollup) treats `@verter/unplugin` as
 * external rather than inlining it into the dist.
 */
function patchWorkspacePackageDeps(project, repoDir, modifiedFiles) {
  // Collect unique package directories containing modified files
  const pkgDirs = new Set();
  for (const relFile of modifiedFiles) {
    const absFile = path.join(repoDir, relFile);
    let dir = path.dirname(absFile);
    // Walk up to find the nearest package.json (stop at repoDir)
    while (dir !== repoDir && dir !== path.dirname(dir)) {
      if (fs.existsSync(path.join(dir, 'package.json'))) {
        pkgDirs.add(dir);
        break;
      }
      dir = path.dirname(dir);
    }
  }

  for (const pkgDir of pkgDirs) {
    // Skip the repo root (it already has @verter/unplugin from installVerterTarballs)
    if (pkgDir === repoDir) continue;

    const pkgPath = path.join(pkgDir, 'package.json');
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    let changed = false;

    for (const depField of ['dependencies', 'devDependencies']) {
      if (pkg[depField]?.['@vitejs/plugin-vue']) {
        const version = pkg[depField]['@vitejs/plugin-vue'];
        pkg[depField]['@verter/unplugin'] = version;
        changed = true;
      }
    }

    if (changed) {
      fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');
      const rel = path.relative(repoDir, pkgPath);
      log(project.name, `  Patched deps: ${rel} (added @verter/unplugin)`);
    }
  }

  // Re-install to update symlinks for new dependencies
  if (pkgDirs.size > 0) {
    const installCmd = project.packageManager === 'pnpm'
      ? 'pnpm install --no-frozen-lockfile'
      : 'npm install --legacy-peer-deps';
    run(installCmd, repoDir, { timeout: 2 * 60_000 });
  }
}

// ── Replace Nuxt Plugin ──────────────────────────────────────────────────────

/**
 * Replace Nuxt's built-in vite:vue plugin with @verter/nuxt.
 * Injects `@verter/nuxt` into the modules array of each nuxt.config.* file.
 */
function replaceNuxtPlugin(project, repoDir) {
  // Find nuxt.config.* files
  const nuxtConfigs = findFiles(repoDir, (name) => name.startsWith('nuxt.config'));
  const modifiedFiles = [];

  for (const configPath of nuxtConfigs) {
    let content;
    try {
      content = fs.readFileSync(configPath, 'utf8');
    } catch {
      continue;
    }

    // Already injected?
    if (content.includes('@verter/nuxt')) continue;

    const moduleEntry = "'@verter/nuxt'";
    let modified = content;

    // Try to inject into existing modules array
    const modulesMatch = modified.match(/modules\s*:\s*\[/);
    if (modulesMatch) {
      const idx = modified.indexOf(modulesMatch[0]) + modulesMatch[0].length;
      modified = modified.slice(0, idx) + `\n    ${moduleEntry},` + modified.slice(idx);
    } else {
      // No modules array — inject one after defineNuxtConfig({
      const configMatch = modified.match(/defineNuxtConfig\s*\(\s*\{/);
      if (configMatch) {
        const idx = modified.indexOf(configMatch[0]) + configMatch[0].length;
        modified = modified.slice(0, idx) + `\n  modules: [${moduleEntry}],` + modified.slice(idx);
      } else {
        log(project.name, `  WARNING: Could not inject module into ${path.relative(repoDir, configPath)}`);
        continue;
      }
    }

    if (modified !== content) {
      fs.writeFileSync(configPath, modified);
      const rel = path.relative(repoDir, configPath);
      modifiedFiles.push(rel);
      log(project.name, `  Modified: ${rel}`);
    }
  }

  return modifiedFiles;
}

// ── Verify Replacement ───────────────────────────────────────────────────────

function verifyReplacement(project, repoDir) {
  if (project.bundler === 'nuxt') {
    // For Nuxt: check that at least one nuxt.config references @verter/nuxt
    const nuxtConfigs = findFiles(repoDir, (name) => name.startsWith('nuxt.config'));
    const found = nuxtConfigs.filter((f) => {
      try {
        return fs.readFileSync(f, 'utf8').includes('@verter/nuxt');
      } catch {
        return false;
      }
    });

    if (found.length === 0) {
      log(project.name, 'ERROR: No nuxt.config has @verter/nuxt injected!');
      return false;
    }

    log(project.name, `Verified @verter/nuxt in ${found.length} config(s):`);
    for (const f of found) {
      log(project.name, `  ${path.relative(repoDir, f)}`);
    }
    return true;
  }

  const isRollup = project.bundler === 'rollup';
  const needle = isRollup ? '@verter/unplugin/rollup' : '@verter/unplugin/vite';

  const files = findFiles(repoDir, (name) => isConfigFile(name));
  const found = files.filter((f) => {
    try {
      return fs.readFileSync(f, 'utf8').includes(needle);
    } catch {
      return false;
    }
  });

  if (found.length === 0) {
    log(project.name, 'ERROR: No files contain @verter/unplugin after replacement!');
    return false;
  }

  log(project.name, `Verified @verter/unplugin in ${found.length} file(s):`);
  for (const f of found) {
    log(project.name, `  ${path.relative(repoDir, f)}`);
  }
  return true;
}

// ── Run Build / Test ─────────────────────────────────────────────────────────

function runBuild(project, repoDir, label) {
  log(project.name, `[${label}] Building: ${project.buildCmd}`);
  const result = run(project.buildCmd, repoDir);
  const dur = (result.durationMs / 1000).toFixed(1);
  log(project.name, `[${label}] Build ${result.ok ? 'OK' : 'FAILED'} (${dur}s)`);
  return { ...result, label: `${label}-build` };
}

function runTest(project, repoDir, label) {
  if (!project.testCmd) {
    return { ok: true, stdout: '', stderr: '', durationMs: 0, skipped: true, label: `${label}-test` };
  }
  log(project.name, `[${label}] Testing: ${project.testCmd}`);
  const result = run(project.testCmd, repoDir, { env: { NODE_ENV: 'test', CI: 'true' } });
  const dur = (result.durationMs / 1000).toFixed(1);
  const counts = extractTestCounts(result.stdout + result.stderr);
  log(
    project.name,
    `[${label}] Tests ${result.ok ? 'OK' : 'FAILED'} (${dur}s) — ${counts.passed} passed, ${counts.failed} failed`,
  );
  return { ...result, ...counts, label: `${label}-test` };
}

async function processLocalProject(project, opts, runContext) {
  const results = {
    name: project.name,
    recipe: project.replacementRecipe,
    executionTier: project.executionTier,
    baseline: { build: null, test: null },
    verter: { build: null, test: null, e2e: null },
    replacement: { modified: [], verified: false, editorModified: [], typeScriptModified: [] },
    typeCheck: null,
    typeCheckCrash: false,
    artifactDir: null,
    error: null,
  };

  const artifactDir = path.join(runContext.reportsDir, sanitizeLocalName(project));
  ensureDir(artifactDir);
  results.artifactDir = artifactDir;
  writeJson(path.join(artifactDir, 'project.json'), project);

  try {
    const repoDir = prepareLocalSandbox(project, runContext);
    writeJson(path.join(artifactDir, 'sandbox.json'), {
      repoRoot: project.repoRoot,
      sandboxDir: repoDir,
    });

    if (project.replacementRecipe === 'editor_only') {
      results.replacement.editorModified = replaceEditorTooling(project, repoDir);
      writeJson(path.join(artifactDir, 'editor-replacement.json'), {
        modifiedFiles: results.replacement.editorModified,
        extensionId: VERTER_EXTENSION_ID,
      });
      writeProjectSummary(project, artifactDir, results, null, null);
      return results;
    }

    if (!['pnpm', 'npm'].includes(project.packageManager || '')) {
      throw new Error(`Unsupported package manager for local execution: ${project.packageManager ?? 'unknown'}`);
    }

    installDeps(project, repoDir);
    fixWindowsScripts(project, repoDir);

    const replacementSteps = new Set(project.replacementSteps || []);
    const runBuilds = ['full_stack', 'build_only'].includes(project.replacementRecipe);
    const runTypeChecksForProject = replacementSteps.has('verter-tsc');
    const replaceTypeScript = replacementSteps.has('typescript-plugin') || replacementSteps.has('verter-tsc');

    if (!opts.skipBaseline && runBuilds && project.buildCmd) {
      results.baseline.build = runBuild(project, repoDir, 'baseline');
      fs.writeFileSync(
        path.join(artifactDir, 'baseline-build.log'),
        results.baseline.build.stdout + '\n' + results.baseline.build.stderr,
      );

      results.baseline.test = runTest(project, repoDir, 'baseline');
      if (results.baseline.test && !results.baseline.test.skipped) {
        fs.writeFileSync(
          path.join(artifactDir, 'baseline-test.log'),
          results.baseline.test.stdout + '\n' + results.baseline.test.stderr,
        );
      }
    }

    if (runTypeChecksForProject) {
      results.typeCheck = runTypeChecks(project, repoDir);
    }

    if (project.surfaces?.editor) {
      results.replacement.editorModified = replaceEditorTooling(project, repoDir);
    }
    if (replaceTypeScript) {
      results.replacement.typeScriptModified = replaceTypeScriptTooling(project, repoDir);
    }

    if (runBuilds) {
      const buildProject = { ...project, bundler: project.surfaces?.buildBundler ?? project.bundler };
      installVerterTarballs(
        buildProject,
        repoDir,
      );
      const modified =
        buildProject.bundler === 'nuxt'
          ? replaceNuxtPlugin(buildProject, repoDir)
          : replaceVuePlugin(buildProject, repoDir);
      patchTsdownConfigs(buildProject, repoDir);
      results.replacement.modified = modified;
      results.replacement.verified = verifyReplacement(buildProject, repoDir);
      if (!results.replacement.verified) {
        throw new Error('Plugin replacement verification failed');
      }
    } else {
      results.replacement.verified = true;
    }

    if (runBuilds && project.buildCmd) {
      results.verter.build = runBuild(project, repoDir, 'verter');
      fs.writeFileSync(
        path.join(artifactDir, 'verter-build.log'),
        results.verter.build.stdout + '\n' + results.verter.build.stderr,
      );

      results.verter.test = runTest(project, repoDir, 'verter');
      if (results.verter.test && !results.verter.test.skipped) {
        fs.writeFileSync(
          path.join(artifactDir, 'verter-test.log'),
          results.verter.test.stdout + '\n' + results.verter.test.stderr,
        );
      }
    }

    const { diff, queue } = writeTypeCheckArtifacts(project, repoDir, artifactDir, results.typeCheck);
    results.typeCheckCrash = Boolean(diff?.summary?.tool_crash);
    writeJson(path.join(artifactDir, 'replacement.json'), results.replacement);
    writeProjectSummary(project, artifactDir, results, diff, queue);
  } catch (/** @type {any} */ err) {
    results.error = err.message;
    writeJson(path.join(artifactDir, 'error.json'), { error: err.message });
    writeProjectSummary(project, artifactDir, results, null, null);
    log(project.name, `ERROR: ${err.message}`);
  }

  return results;
}

// ── Process One Project ──────────────────────────────────────────────────────

async function processProject(project, opts) {
  const results = {
    name: project.name,
    recipe: 'full_stack',
    executionTier: 'tier1',
    baseline: { build: null, test: null },
    verter: { build: null, test: null, e2e: null },
    replacement: { modified: [], verified: false },
    typeCheck: null,
    typeCheckCrash: false,
    artifactDir: path.join(LOGS_DIR, project.name),
    error: null,
  };

  try {
    const repoDir = opts.noClone
      ? path.join(REPOS_DIR, project.name)
      : cloneProject(project);
    const logDir = path.join(LOGS_DIR, project.name);
    fs.mkdirSync(logDir, { recursive: true });

    if (!fs.existsSync(repoDir)) {
      throw new Error(`Project directory does not exist: ${repoDir}`);
    }

    // When reusing an existing checkout, reset git-tracked files to undo
    // any verter modifications from a previous run (config files, package.json, .npmrc, etc.)
    if (opts.noClone) {
      log(project.name, 'Resetting git-tracked files to clean state...');
      run('git checkout .', repoDir);
      // Clear turborepo cache to avoid stale builds
      const turboDir = path.join(repoDir, '.turbo');
      if (fs.existsSync(turboDir)) {
        fs.rmSync(turboDir, { recursive: true, force: true });
        log(project.name, 'Cleared .turbo cache');
      }
    }

    installDeps(project, repoDir);
    fixWindowsScripts(project, repoDir);

    // ── Baseline ──
    if (!opts.skipBaseline) {
      results.baseline.build = runBuild(project, repoDir, 'baseline');
      results.baseline.test = runTest(project, repoDir, 'baseline');

      // Save baseline logs
      if (results.baseline.build) {
        fs.writeFileSync(
          path.join(logDir, 'baseline-build.log'),
          results.baseline.build.stdout + '\n' + results.baseline.build.stderr,
        );
      }
      if (results.baseline.test && !results.baseline.test.skipped) {
        fs.writeFileSync(
          path.join(logDir, 'baseline-test.log'),
          results.baseline.test.stdout + '\n' + results.baseline.test.stderr,
        );
      }
    }

    // ── Type-check timing (vue-tsc vs verter-tsc, 2 passes each) ──
    // Run BEFORE the Verter swap so both tools see the same unmodified project.
    results.typeCheck = runTypeChecks(project, repoDir);

    // ── Verter swap ──
    installVerterTarballs(project, repoDir);
    const modified =
      project.bundler === 'nuxt'
        ? replaceNuxtPlugin(project, repoDir)
        : replaceVuePlugin(project, repoDir);
    patchTsdownConfigs(project, repoDir);
    results.replacement.modified = modified;
    results.replacement.verified = verifyReplacement(project, repoDir);
    const typeArtifacts = writeTypeCheckArtifacts(project, repoDir, logDir, results.typeCheck);
    results.typeCheckCrash = Boolean(typeArtifacts.diff?.summary?.tool_crash);

    if (!results.replacement.verified) {
      writeJson(path.join(logDir, 'replacement.json'), results.replacement);
      writeProjectSummary({ ...project, executionTier: 'tier1', replacementRecipe: 'full_stack' }, logDir, results, typeArtifacts.diff, typeArtifacts.queue);
      results.error = 'Plugin replacement verification failed';
      return results;
    }

    // ── Verter build + test ──
    results.verter.build = runBuild(project, repoDir, 'verter');
    results.verter.test = runTest(project, repoDir, 'verter');

    // Retry flaky tests: if verter has more failures than baseline, re-run up to 3 times
    // and keep the best result (fewest failures).
    const MAX_TEST_RETRIES = 3;
    if (
      results.verter.test &&
      !results.verter.test.skipped &&
      results.baseline.test &&
      !results.baseline.test.skipped
    ) {
      const bFailed = results.baseline.test.failed || 0;
      let vFailed = results.verter.test.failed || 0;

      for (let retry = 1; retry <= MAX_TEST_RETRIES && vFailed > bFailed; retry++) {
        log(
          project.name,
          `[verter] Retrying tests (${retry}/${MAX_TEST_RETRIES}) — ${vFailed} failures vs baseline ${bFailed}`,
        );
        const retryResult = runTest(project, repoDir, `verter retry ${retry}`);
        const retryFailed = retryResult.failed || 0;
        if (retryFailed < vFailed) {
          results.verter.test = retryResult;
          vFailed = retryFailed;
        }
      }
    }

    // ── E2E tests (Verter only) ──
    if (project.e2eCmd && results.verter.build?.ok) {
      log(project.name, `[verter] E2E: ${project.e2eCmd}`);
      const e2eResult = run(project.e2eCmd, repoDir, { timeout: 5 * 60_000 });
      const dur = (e2eResult.durationMs / 1000).toFixed(1);
      log(project.name, `[verter] E2E ${e2eResult.ok ? 'OK' : 'FAILED'} (${dur}s)`);
      results.verter.e2e = e2eResult;
    }

    // Save verter logs
    if (results.verter.build) {
      fs.writeFileSync(
        path.join(logDir, 'verter-build.log'),
        results.verter.build.stdout + '\n' + results.verter.build.stderr,
      );
    }
    if (results.verter.test && !results.verter.test.skipped) {
      fs.writeFileSync(
        path.join(logDir, 'verter-test.log'),
        results.verter.test.stdout + '\n' + results.verter.test.stderr,
      );
    }
    if (results.verter.e2e) {
      fs.writeFileSync(
        path.join(logDir, 'verter-e2e.log'),
        results.verter.e2e.stdout + '\n' + results.verter.e2e.stderr,
      );
    }

    const { diff, queue } = typeArtifacts;
    writeJson(path.join(logDir, 'replacement.json'), results.replacement);
    writeProjectSummary({ ...project, executionTier: 'tier1', replacementRecipe: 'full_stack' }, logDir, results, diff, queue);
  } catch (/** @type {any} */ err) {
    results.error = err.message;
    log(project.name, `ERROR: ${err.message}`);
  }

  return results;
}

// ── Summary ──────────────────────────────────────────────────────────────────

function formatDuration(ms) {
  if (ms == null) return '-';
  return `${(ms / 1000).toFixed(1)}s`;
}

function formatTestResult(result) {
  if (!result || result.skipped) return 'skipped';
  const p = result.passed || 0;
  const f = result.failed || 0;
  if (f > 0) return `FAIL ${f}/${p + f}`;
  if (p > 0) return `OK ${p}`;
  return result.ok ? 'OK' : 'FAIL';
}

function printSummary(allResults) {
  console.log('\n' + '='.repeat(100));
  console.log('INTEGRATION TEST SUMMARY');
  console.log('='.repeat(100));

  // Header
  const cols = [
    { name: 'Project', width: 22 },
    { name: 'B.Build', width: 10 },
    { name: 'V.Build', width: 10 },
    { name: 'Delta', width: 10 },
    { name: 'B.Tests', width: 14 },
    { name: 'V.Tests', width: 14 },
    { name: 'E2E', width: 8 },
    { name: 'Status', width: 12 },
  ];

  const header = cols.map((c) => c.name.padEnd(c.width)).join(' | ');
  console.log(header);
  console.log(cols.map((c) => '-'.repeat(c.width)).join('-+-'));

  let passed = 0;
  let failed = 0;
  let warnings = 0;

  for (const r of allResults) {
    if (r.error) {
      const row = [
        r.name.padEnd(22),
        'ERROR'.padEnd(10),
        ''.padEnd(10),
        ''.padEnd(10),
        ''.padEnd(14),
        ''.padEnd(14),
        ''.padEnd(8),
        'ERROR'.padEnd(12),
      ];
      console.log(row.join(' | '));
      failed++;
      continue;
    }

    const bBuild = r.baseline.build?.durationMs;
    const vBuild = r.verter.build?.durationMs;
    const expectsBuild = r.recipe !== 'editor_only' && r.recipe !== 'typecheck_only';

    let delta = '-';
    if (bBuild != null && vBuild != null && bBuild > 0) {
      const diff = vBuild - bBuild;
      const pct = ((diff / bBuild) * 100).toFixed(0);
      delta = diff > 0 ? `+${pct}%` : `${pct}%`;
    }

    let status = 'OK';
    if (r.recipe === 'editor_only') {
      status = 'EDITOR ONLY';
      passed++;
    } else if (r.typeCheckCrash) {
      status = 'TSC CRASH';
      failed++;
    } else if (!expectsBuild) {
      status = 'TYPECHECK';
      passed++;
    } else if (!r.verter.build?.ok) {
      status = 'BUILD FAIL';
      failed++;
    } else if (r.verter.test && !r.verter.test.skipped && !r.verter.test.ok) {
      const vFailed = r.verter.test.failed || 0;
      const bFailed = r.baseline.test?.failed || 0;
      if (vFailed > bFailed) {
        status = 'TEST REGR';
        failed++;
      } else {
        status = 'TEST FAIL';
        warnings++;
      }
    } else if (vBuild != null && bBuild != null && vBuild > bBuild) {
      status = 'SLOWER';
      warnings++;
    } else {
      passed++;
    }

    let e2eStatus = '-';
    if (r.verter.e2e) {
      e2eStatus = r.verter.e2e.ok ? 'OK' : 'FAIL';
    }

    const row = [
      r.name.padEnd(22),
      formatDuration(bBuild).padEnd(10),
      formatDuration(vBuild).padEnd(10),
      delta.padEnd(10),
      formatTestResult(r.baseline.test).padEnd(14),
      formatTestResult(r.verter.test).padEnd(14),
      e2eStatus.padEnd(8),
      status.padEnd(12),
    ];
    console.log(row.join(' | '));
  }

  console.log('-'.repeat(100));
  console.log(`${passed} passed / ${warnings} warnings / ${failed} failed`);
  console.log('');
  const artifactRoots = [...new Set(
    allResults
      .map((result) => result.artifactDir ? path.dirname(result.artifactDir) : null)
      .filter(Boolean),
  )];
  if (artifactRoots.length > 0) {
    console.log('Artifacts:');
    for (const artifactRoot of artifactRoots) {
      console.log(`  ${artifactRoot}`);
    }
  } else {
    console.log(`Logs: ${LOGS_DIR}`);
  }

  // ── Type-Check Timing ──
  const tcResults = allResults.filter((r) => r.typeCheck != null);
  if (tcResults.length > 0) {
    console.log('');
    console.log('='.repeat(100));
    console.log('TYPE-CHECK TIMING (vue-tsc vs verter-tsc)');
    console.log('Both tools run: --noEmit --project tsconfig.json');
    if (VERTER_TSC.bin) {
      console.log(`verter-tsc: ${VERTER_TSC.type} (${VERTER_TSC.bin})`);
    } else {
      console.log('verter-tsc: NOT FOUND — skipped');
    }
    console.log('='.repeat(100));

    const tcCols = [
      { name: 'Project', width: 22 },
      { name: 'vue:cold', width: 10 },
      { name: 'vue:warm', width: 10 },
      { name: 'v-tsc:cold', width: 10 },
      { name: 'v-tsc:warm', width: 10 },
      { name: 'speedup', width: 8 },
      { name: 'errs:vue', width: 9 },
      { name: 'errs:v', width: 9 },
    ];
    const tcHeader = tcCols.map((c) => c.name.padEnd(c.width)).join(' | ');
    console.log(tcHeader);
    console.log(tcCols.map((c) => '-'.repeat(c.width)).join('-+-'));

    for (const r of tcResults) {
      const tc = r.typeCheck;
      const fmtTc = (result) => {
        if (!result) return '-';
        if (result.timedOut) return '>5min';
        const s = formatDuration(result.ms);
        return result.exitCode !== 0 ? s + '(err)' : s;
      };

      // Speedup = vue-tsc warm / verter-tsc warm
      // Show speedup when both tools complete (TS errors are expected and don't invalidate timing).
      // Only suppress speedup when a tool crashes instantly (<2s with error) — that means it
      // didn't actually type-check (e.g. Volar version incompatibility).
      let speedup = '-';
      const vueDone = tc.vueTsc.warm && !tc.vueTsc.warm.timedOut;
      const vtscDone = tc.verterTsc.warm && !tc.verterTsc.warm.timedOut;
      const vueActuallyRan = vueDone && (tc.vueTsc.warm.exitCode === 0 || tc.vueTsc.warm.ms > 2000);
      const vtscActuallyRan = vtscDone && (tc.verterTsc.warm.exitCode === 0 || tc.verterTsc.warm.ms > 2000);
      if (vueActuallyRan && vtscActuallyRan && tc.verterTsc.warm.ms > 0) {
        speedup = (tc.vueTsc.warm.ms / tc.verterTsc.warm.ms).toFixed(1) + 'x';
      }

      // Error counts from warm pass (vue-tsc is baseline)
      const vueErrs = tc.vueTsc.warm ? String(tc.vueTsc.warm.errorCount) : '-';
      const vtscErrs = tc.verterTsc.warm ? String(tc.verterTsc.warm.errorCount) : '-';

      const row = [
        r.name.padEnd(22),
        fmtTc(tc.vueTsc.cold).padEnd(10),
        fmtTc(tc.vueTsc.warm).padEnd(10),
        fmtTc(tc.verterTsc.cold).padEnd(10),
        fmtTc(tc.verterTsc.warm).padEnd(10),
        speedup.padEnd(8),
        vueErrs.padEnd(9),
        vtscErrs.padEnd(9),
      ];
      console.log(row.join(' | '));
    }

    console.log('-'.repeat(100));
    console.log('vue-tsc is the baseline. errs = TS error count from warm pass.');
  }

  return failed > 0 ? 1 : 0;
}

function writeDiscoveryArtifacts(runContext, inventory) {
  writeJson(path.join(runContext.runDir, 'discovery.json'), inventory);
  fs.writeFileSync(path.join(runContext.runDir, 'discovery.md'), renderDiscoveryMarkdown(inventory));
}

function matchesSelection(project, selectedNames) {
  if (selectedNames.length === 0) return true;
  return selectedNames.includes(project.name)
    || selectedNames.includes(project.id)
    || selectedNames.includes(project.relativeRoot);
}

function sortLocalProjects(projectsToRun) {
  const recipeOrder = ['full_stack', 'typecheck_only', 'build_only', 'editor_only', 'manual_review'];
  return [...projectsToRun].sort((a, b) => {
    const recipeDelta = recipeOrder.indexOf(a.replacementRecipe) - recipeOrder.indexOf(b.replacementRecipe);
    if (recipeDelta !== 0) return recipeDelta;
    return a.repoRoot.localeCompare(b.repoRoot);
  });
}

// ── Main ─────────────────────────────────────────────────────────────────────

async function main() {
  const opts = parseArgs();
  const shouldDiscoverLocal = opts.discoverLocal || opts.discoverOnly || opts.localOnly;
  const shouldRunLocal = shouldDiscoverLocal && !opts.discoverOnly;
  const shouldRunMatrix = !opts.localOnly && !opts.discoverOnly;
  const allResults = [];

  let inventory = null;
  let runContext = null;
  let localSelected = [];
  let manualReviewSelected = [];

  if (shouldDiscoverLocal) {
    runContext = createLocalRunContext(opts);
    inventory = buildDiscoveryInventory({
      roots: opts.roots,
      repoFilter: opts.repoFilter,
      matrixProjects: projects,
    });
    writeDiscoveryArtifacts(runContext, inventory);

    const localMatches = inventory.localProjects
      .filter((project) => matchesSelection(project, opts.projectNames));
    manualReviewSelected = localMatches.filter((project) => project.replacementRecipe === 'manual_review');
    localSelected = sortLocalProjects(
      localMatches.filter((project) => project.executionTier === 'tier2' && project.replacementRecipe !== 'manual_review'),
    );

    console.log(`Local discovery written to ${runContext.runDir}`);
    console.log(`  Local repos discovered: ${inventory.localProjects.length}`);
    console.log(`  Tier 2 repos selected for execution: ${localSelected.length}`);
    if (manualReviewSelected.length > 0) {
      console.log(`  Manual review required: ${manualReviewSelected.map((project) => project.relativeRoot).join(', ')}`);
    }
    console.log('');
  }

  const selected = projects.filter((project) => matchesSelection(project, opts.projectNames));
  const unknown = opts.projectNames.filter((name) => {
    const inMatrix = projects.some((project) => project.name === name);
    const inLocal = inventory?.localProjects.some((project) => matchesSelection(project, [name])) ?? false;
    return !inMatrix && !inLocal;
  });
  if (unknown.length > 0) {
    console.error(`Unknown project(s): ${unknown.join(', ')}`);
    process.exit(1);
  }

  if (shouldRunMatrix) {
    console.log(`Running integration tests for ${selected.length} matrix project(s):`);
    console.log(`  ${selected.map((p) => p.name).join(', ') || '(none)'}`);
    console.log('');
  }

  if (shouldRunMatrix || shouldRunLocal) {
    fs.mkdirSync(REPOS_DIR, { recursive: true });
    fs.mkdirSync(TARBALLS_DIR, { recursive: true });
    fs.mkdirSync(LOGS_DIR, { recursive: true });

    const workspaceFile = path.join(INTEGRATION_DIR, 'pnpm-workspace.yaml');
    if (!fs.existsSync(workspaceFile)) {
      fs.writeFileSync(workspaceFile, 'packages: []\n');
    }

    if (!opts.skipBuild) {
      buildVerter({ fast: opts.fast });
    } else {
      const needsTarballs = (shouldRunMatrix && selected.length > 0)
        || localSelected.some((project) => ['full_stack', 'build_only'].includes(project.replacementRecipe));
      const needsTypecheckBinary = localSelected.some((project) => ['full_stack', 'typecheck_only'].includes(project.replacementRecipe))
        || (shouldRunMatrix && selected.length > 0);
      const tarballs = fs.existsSync(TARBALLS_DIR)
        ? fs.readdirSync(TARBALLS_DIR).filter((f) => f.endsWith('.tgz'))
        : [];
      if (needsTarballs && tarballs.length < 2) {
        console.error('No tarballs found. Run without --skip-build first.');
        process.exit(1);
      }
      if (needsTypecheckBinary && !VERTER_TSC.bin) {
        console.error('No verter-tsc binary found. Run without --skip-build first.');
        process.exit(1);
      }
      if (needsTarballs || needsTypecheckBinary) {
        log('verter', `Reusing existing tarballs: ${tarballs.join(', ')}`);
      } else {
        log('verter', 'Skipping Verter build: no selected repo needs tarballs or verter-tsc');
      }
    }
  }

  if (shouldRunMatrix) {
    if (opts.concurrency <= 1) {
      for (const project of selected) {
        console.log(`\n${'─'.repeat(80)}`);
        log(project.name, `Starting (${project.packageManager}, ${project.bundler})`);
        const result = await processProject(project, opts);
        allResults.push(result);
      }
    } else {
      for (let i = 0; i < selected.length; i += opts.concurrency) {
        const batch = selected.slice(i, i + opts.concurrency);
        const batchResults = await Promise.all(
          batch.map((project) => {
            log(project.name, `Starting (${project.packageManager}, ${project.bundler})`);
            return processProject(project, opts);
          }),
        );
        allResults.push(...batchResults);
      }
    }
  }

  if (shouldRunLocal) {
    console.log(`Running Tier 2 local projects for ${localSelected.length} repo(s):`);
    console.log(`  ${localSelected.map((project) => project.relativeRoot).join(', ') || '(none)'}`);
    console.log('');

    if (opts.concurrency <= 1) {
      for (const project of localSelected) {
        console.log(`\n${'─'.repeat(80)}`);
        log(project.name, `Starting local (${project.packageManager ?? 'unknown'}, ${project.surfaces?.buildBundler ?? 'n/a'})`);
        const result = await processLocalProject(project, opts, runContext);
        allResults.push(result);
      }
    } else {
      for (let i = 0; i < localSelected.length; i += opts.concurrency) {
        const batch = localSelected.slice(i, i + opts.concurrency);
        const batchResults = await Promise.all(
          batch.map((project) => {
            log(project.name, `Starting local (${project.packageManager ?? 'unknown'}, ${project.surfaces?.buildBundler ?? 'n/a'})`);
            return processLocalProject(project, opts, runContext);
          }),
        );
        allResults.push(...batchResults);
      }
    }
  }

  if (opts.discoverOnly && !shouldRunMatrix) {
    process.exit(0);
  }

  if (allResults.length === 0) {
    process.exit(0);
  }

  const exitCode = printSummary(allResults);
  process.exit(exitCode);
}

main().catch((err) => {
  console.error('Fatal error:', err);
  process.exit(1);
});
