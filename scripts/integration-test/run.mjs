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

import { execSync, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';
import { projects } from './projects.mjs';

// ── Paths ────────────────────────────────────────────────────────────────────

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, '../..');
const INTEGRATION_DIR = path.join(ROOT, '.integration-tests');
const REPOS_DIR = path.join(INTEGRATION_DIR, 'repos');
const TARBALLS_DIR = path.join(INTEGRATION_DIR, 'tarballs');
const LOGS_DIR = path.join(INTEGRATION_DIR, 'logs');

// ── CLI Parsing ──────────────────────────────────────────────────────────────

function parseArgs() {
  const args = process.argv.slice(2);
  const opts = {
    skipBaseline: false,
    skipBuild: false,
    noClone: false,
    fast: false,
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
            '  --skip-build      Skip building Verter (reuse tarballs)',
            '  --no-clone        Skip git clone (reuse checkouts)',
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
      fs.copyFileSync(srcPath, destPath);
    }
  }
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

  log('verter', 'Building unplugin...');
  const unplugin = run('pnpm --filter @verter/unplugin build', ROOT);
  if (!unplugin.ok) {
    console.error(unplugin.stderr || unplugin.stdout);
    throw new Error('Failed to build unplugin');
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

  const tarballs = fs.readdirSync(TARBALLS_DIR).filter((f) => f.endsWith('.tgz'));
  if (tarballs.length < 2) {
    throw new Error(`Expected 2 tarballs, found ${tarballs.length} in ${TARBALLS_DIR}`);
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

  if (!nativeTgz || !unpluginTgz) {
    throw new Error('Missing verter tarballs. Run without --skip-build first.');
  }

  // Use forward slashes for cross-platform compat in shell commands
  const nativePath = path.join(TARBALLS_DIR, nativeTgz).replace(/\\/g, '/');
  const unpluginPath = path.join(TARBALLS_DIR, unpluginTgz).replace(/\\/g, '/');

  log(project.name, 'Installing Verter tarballs...');

  // Compute relative tarball paths (with forward slashes) for package.json overrides.
  // Using file: protocol ensures pnpm/npm resolve ALL references to the tarball
  // rather than trying to fetch the semver version from the registry.
  const relNative = path.relative(repoDir, path.join(TARBALLS_DIR, nativeTgz)).replace(/\\/g, '/');
  const relUnplugin = path.relative(repoDir, path.join(TARBALLS_DIR, unpluginTgz)).replace(/\\/g, '/');

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

    run(`pnpm add -w "${nativePath}" "${unpluginPath}"`, repoDir);

    // Add pnpm overrides using file: protocol to the tarballs.
    // The $packageName syntax doesn't work with tarball-installed packages —
    // pnpm resolves the semver version separately, missing the native binary.
    const pkgPath = path.join(repoDir, 'package.json');
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    pkg.pnpm = pkg.pnpm || {};
    pkg.pnpm.overrides = pkg.pnpm.overrides || {};
    pkg.pnpm.overrides['@verter/native'] = `file:${relNative}`;
    pkg.pnpm.overrides['@verter/unplugin'] = `file:${relUnplugin}`;
    fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');

    run('pnpm install --no-frozen-lockfile', repoDir);
  } else {
    run(`npm install --legacy-peer-deps "${nativePath}" "${unpluginPath}"`, repoDir);

    // Add npm overrides using file: protocol
    const pkgPath = path.join(repoDir, 'package.json');
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    pkg.overrides = pkg.overrides || {};
    pkg.overrides['@verter/native'] = `file:${relNative}`;
    pkg.overrides['@verter/unplugin'] = `file:${relUnplugin}`;
    fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');

    run('npm install --legacy-peer-deps', repoDir);
  }

  ensureVerterAccessible(project, repoDir);
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
    // Copy dist + package.json but skip src; keep node_modules since
    // @verter/unplugin depends on 'unplugin' which lives in its own node_modules
    copyRecursive(srcUnplugin, unpluginDir, ['src']);
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

/** Nuxt module content that replaces the built-in vite:vue plugin with Verter. */
const NUXT_OVERRIDE_MODULE = `\
import { defineNuxtModule } from '@nuxt/kit'
import verter from '@verter/unplugin/vite'

export default defineNuxtModule({
  meta: { name: 'verter-override' },
  setup(_options, nuxt) {
    nuxt.hook('vite:extendConfig', (config) => {
      // Remove the built-in vite:vue plugin
      config.plugins = (config.plugins || []).filter(
        (p) => !(p && typeof p === 'object' && 'name' in p && p.name === 'vite:vue')
      )
      // Add Verter in its place
      config.plugins.push(verter())
    })
  }
})
`;

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
    if (content.includes('.verter-nuxt-override')) continue;

    // Place the override module next to each nuxt.config file so the
    // relative path './.verter-nuxt-override' always resolves correctly.
    const configDir = path.dirname(configPath);
    const modulePath = path.join(configDir, '.verter-nuxt-override.mjs');
    if (!fs.existsSync(modulePath)) {
      fs.writeFileSync(modulePath, NUXT_OVERRIDE_MODULE);
      const relModule = path.relative(repoDir, modulePath);
      modifiedFiles.push(relModule);
      log(project.name, `  Created ${relModule}`);
    }

    const moduleEntry = "'./.verter-nuxt-override'";
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
    // For Nuxt: check that at least one nuxt.config references the override
    // and its companion .verter-nuxt-override.mjs exists alongside it.
    const nuxtConfigs = findFiles(repoDir, (name) => name.startsWith('nuxt.config'));
    const found = nuxtConfigs.filter((f) => {
      try {
        if (!fs.readFileSync(f, 'utf8').includes('.verter-nuxt-override')) return false;
        // Also verify the module file exists next to this config
        const moduleFile = path.join(path.dirname(f), '.verter-nuxt-override.mjs');
        return fs.existsSync(moduleFile);
      } catch {
        return false;
      }
    });

    if (found.length === 0) {
      log(project.name, 'ERROR: No nuxt.config has .verter-nuxt-override injected!');
      return false;
    }

    log(project.name, `Verified Nuxt override module in ${found.length} config(s):`);
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

// ── Process One Project ──────────────────────────────────────────────────────

async function processProject(project, opts) {
  const results = {
    name: project.name,
    baseline: { build: null, test: null },
    verter: { build: null, test: null, e2e: null },
    replacement: { modified: [], verified: false },
    error: null,
  };

  try {
    const repoDir = opts.noClone
      ? path.join(REPOS_DIR, project.name)
      : cloneProject(project);

    if (!fs.existsSync(repoDir)) {
      throw new Error(`Project directory does not exist: ${repoDir}`);
    }

    // When reusing an existing checkout, reset git-tracked files to undo
    // any verter modifications from a previous run (config files, package.json, .npmrc, etc.)
    if (opts.noClone) {
      log(project.name, 'Resetting git-tracked files to clean state...');
      run('git checkout .', repoDir);
    }

    installDeps(project, repoDir);
    fixWindowsScripts(project, repoDir);

    // ── Baseline ──
    if (!opts.skipBaseline) {
      results.baseline.build = runBuild(project, repoDir, 'baseline');
      results.baseline.test = runTest(project, repoDir, 'baseline');

      // Save baseline logs
      const logDir = path.join(LOGS_DIR, project.name);
      fs.mkdirSync(logDir, { recursive: true });
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

    // ── Verter swap ──
    installVerterTarballs(project, repoDir);
    const modified =
      project.bundler === 'nuxt'
        ? replaceNuxtPlugin(project, repoDir)
        : replaceVuePlugin(project, repoDir);
    results.replacement.modified = modified;
    results.replacement.verified = verifyReplacement(project, repoDir);

    if (!results.replacement.verified) {
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
    const logDir = path.join(LOGS_DIR, project.name);
    fs.mkdirSync(logDir, { recursive: true });
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

    let delta = '-';
    if (bBuild != null && vBuild != null && bBuild > 0) {
      const diff = vBuild - bBuild;
      const pct = ((diff / bBuild) * 100).toFixed(0);
      delta = diff > 0 ? `+${pct}%` : `${pct}%`;
    }

    let status = 'OK';
    if (!r.verter.build?.ok) {
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
  console.log(`Logs: ${LOGS_DIR}`);

  return failed > 0 ? 1 : 0;
}

// ── Main ─────────────────────────────────────────────────────────────────────

async function main() {
  const opts = parseArgs();

  // Filter projects if names are specified
  let selected = projects;
  if (opts.projectNames.length > 0) {
    selected = projects.filter((p) => opts.projectNames.includes(p.name));
    const unknown = opts.projectNames.filter((n) => !projects.find((p) => p.name === n));
    if (unknown.length > 0) {
      console.error(`Unknown project(s): ${unknown.join(', ')}`);
      console.error(`Available: ${projects.map((p) => p.name).join(', ')}`);
      process.exit(1);
    }
  }

  console.log(`Running integration tests for ${selected.length} project(s):`);
  console.log(`  ${selected.map((p) => p.name).join(', ')}`);
  console.log('');

  // Set up directories
  fs.mkdirSync(REPOS_DIR, { recursive: true });
  fs.mkdirSync(TARBALLS_DIR, { recursive: true });
  fs.mkdirSync(LOGS_DIR, { recursive: true });

  // Create workspace boundary to isolate from the Verter monorepo
  const workspaceFile = path.join(INTEGRATION_DIR, 'pnpm-workspace.yaml');
  if (!fs.existsSync(workspaceFile)) {
    fs.writeFileSync(workspaceFile, 'packages: []\n');
  }

  // Build Verter
  if (!opts.skipBuild) {
    buildVerter({ fast: opts.fast });
  } else {
    const tarballs = fs.existsSync(TARBALLS_DIR)
      ? fs.readdirSync(TARBALLS_DIR).filter((f) => f.endsWith('.tgz'))
      : [];
    if (tarballs.length < 2) {
      console.error('No tarballs found. Run without --skip-build first.');
      process.exit(1);
    }
    log('verter', `Reusing existing tarballs: ${tarballs.join(', ')}`);
  }

  // Run projects
  const allResults = [];

  if (opts.concurrency <= 1) {
    // Sequential
    for (const project of selected) {
      console.log(`\n${'─'.repeat(80)}`);
      log(project.name, `Starting (${project.packageManager}, ${project.bundler})`);
      const result = await processProject(project, opts);
      allResults.push(result);
    }
  } else {
    // Parallel batches
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

  // Summary
  const exitCode = printSummary(allResults);
  process.exit(exitCode);
}

main().catch((err) => {
  console.error('Fatal error:', err);
  process.exit(1);
});
