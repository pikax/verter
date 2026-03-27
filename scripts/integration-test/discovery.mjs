import fs from "node:fs";
import path from "node:path";

export const DISCOVERY_SCHEMA = "verter.discovery.v1";
export const VERTER_EXTENSION_ID = "pikax.verter-vscode";

export const EXCLUDED_DIR_NAMES = new Set([
  ".git",
  ".hg",
  ".svn",
  ".turbo",
  ".next",
  ".nuxt",
  ".output",
  ".vercel",
  ".cache",
  "coverage",
  "dist",
  "build",
  "out",
  "target",
  "node_modules",
  "tmp",
  "temp",
  "vendor",
  "__pycache__",
]);

const ROOT_TS_CONFIG_NAMES = [
  "tsconfig.json",
  "tsconfig.web.json",
  "tsconfig.app.json",
  "tsconfig.src.json",
  "jsconfig.json",
];

const WORKSPACE_FILE_NAMES = new Set(["settings.json", "extensions.json"]);
const LOCKFILE_NAMES = [
  "pnpm-lock.yaml",
  "package-lock.json",
  "yarn.lock",
  "bun.lock",
  "bun.lockb",
];
const CONFIG_PREFIXES = ["vite.config", "rollup.config", "nuxt.config", "vitest.config"];

function normalizePath(value) {
  return value.replace(/\\/g, "/");
}

function safeRead(filePath) {
  try {
    return fs.readFileSync(filePath, "utf8");
  } catch {
    return "";
  }
}

function tryParseJson(raw) {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

function isVerterToolchainName(name) {
  return (
    typeof name === "string" &&
    (name === "verter" || name === "verter-vscode" || name.startsWith("@verter/"))
  );
}

function countBy(items, key) {
  const counts = {};
  for (const item of items) {
    const value = item[key] ?? "unknown";
    counts[value] = (counts[value] || 0) + 1;
  }
  return counts;
}

function isWorkspaceFile(fullPath) {
  if (fullPath.endsWith(".code-workspace")) return true;
  const base = path.basename(fullPath);
  if (!WORKSPACE_FILE_NAMES.has(base)) return false;
  return normalizePath(fullPath).includes("/.vscode/");
}

function isTsConfigFile(name) {
  return name === "jsconfig.json" || /^tsconfig(\..+)?\.json$/u.test(name);
}

function isBuildConfigFile(name) {
  return CONFIG_PREFIXES.some((prefix) => name.startsWith(prefix));
}

function shouldSkipDir(dirName, relativePath) {
  if (EXCLUDED_DIR_NAMES.has(dirName)) return true;
  const normalized = normalizePath(relativePath);
  if (normalized.includes("/.integration-tests/repos/")) return true;
  if (normalized.includes("/vendor/")) return true;
  return false;
}

function detectPackageManager(repoRoot, rootPackageRaw) {
  if (rootPackageRaw) {
    const pkg = tryParseJson(rootPackageRaw);
    const packageManager = typeof pkg?.packageManager === "string" ? pkg.packageManager : "";
    if (packageManager.startsWith("pnpm@"))
      return { packageManager: "pnpm", lockfile: "pnpm-lock.yaml" };
    if (packageManager.startsWith("npm@"))
      return { packageManager: "npm", lockfile: "package-lock.json" };
    if (packageManager.startsWith("yarn@"))
      return { packageManager: "yarn", lockfile: "yarn.lock" };
    if (packageManager.startsWith("bun@")) return { packageManager: "bun", lockfile: "bun.lock" };
  }

  for (const lockfile of LOCKFILE_NAMES) {
    if (fs.existsSync(path.join(repoRoot, lockfile))) {
      if (lockfile === "pnpm-lock.yaml") return { packageManager: "pnpm", lockfile };
      if (lockfile === "package-lock.json") return { packageManager: "npm", lockfile };
      if (lockfile === "yarn.lock") return { packageManager: "yarn", lockfile };
      return { packageManager: "bun", lockfile };
    }
  }

  return { packageManager: null, lockfile: null };
}

function findGitRepos(rootDir, repoFilter) {
  const results = [];
  const seen = new Set();

  function walk(currentDir) {
    let entries;
    try {
      entries = fs.readdirSync(currentDir, { withFileTypes: true });
    } catch {
      return;
    }

    if (entries.some((entry) => entry.name === ".git")) {
      const normalized = normalizePath(currentDir);
      if ((!repoFilter || repoFilter.test(normalized)) && !seen.has(normalized)) {
        seen.add(normalized);
        results.push(currentDir);
      }
      return;
    }

    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const fullPath = path.join(currentDir, entry.name);
      const relative = path.relative(rootDir, fullPath);
      if (shouldSkipDir(entry.name, relative)) continue;
      walk(fullPath);
    }
  }

  walk(rootDir);
  return results.sort((a, b) => a.localeCompare(b));
}

function scanRepo(repoRoot) {
  const packageJsonFiles = [];
  const workspaceFiles = [];
  const tsconfigFiles = [];
  const configFiles = [];
  const vueFiles = [];
  let vueFileCount = 0;

  function walk(currentDir) {
    let entries;
    try {
      entries = fs.readdirSync(currentDir, { withFileTypes: true });
    } catch {
      return;
    }

    for (const entry of entries) {
      const fullPath = path.join(currentDir, entry.name);
      const relative = path.relative(repoRoot, fullPath);
      if (entry.isDirectory()) {
        if (shouldSkipDir(entry.name, relative)) continue;
        walk(fullPath);
        continue;
      }

      if (entry.name === "package.json") {
        packageJsonFiles.push(fullPath);
      } else if (isWorkspaceFile(fullPath)) {
        workspaceFiles.push(fullPath);
      } else if (isTsConfigFile(entry.name)) {
        tsconfigFiles.push(fullPath);
      } else if (isBuildConfigFile(entry.name)) {
        configFiles.push(fullPath);
      } else if (entry.name.endsWith(".vue")) {
        vueFileCount += 1;
        if (vueFiles.length < 10) vueFiles.push(fullPath);
      }
    }
  }

  walk(repoRoot);

  return {
    packageJsonFiles: packageJsonFiles.sort(),
    workspaceFiles: workspaceFiles.sort(),
    tsconfigFiles: tsconfigFiles.sort(),
    configFiles: configFiles.sort(),
    vueFiles: vueFiles.sort(),
    vueFileCount,
  };
}

function extractSurfaceFlags(repoRoot, scan, rootPackageRaw) {
  let hasVueTsc = false;
  let hasVueTypeScriptPlugin = false;
  let hasVueLanguageTools = false;
  let hasViteVue = false;
  let hasRollupVue = false;
  let hasNuxt = false;
  let hasVueDependency = false;
  const packages = [];
  const buildSurfaceKinds = new Set();
  const buildSurfaceMatches = [];
  const rootPackage = tryParseJson(rootPackageRaw);

  for (const packageJson of scan.packageJsonFiles) {
    const raw = safeRead(packageJson);
    const pkg = tryParseJson(raw);
    const relPath = normalizePath(path.relative(repoRoot, packageJson));

    const mentionsVue = /"vue"\s*:/u.test(raw) || /"nuxt"\s*:/u.test(raw);
    if (mentionsVue) hasVueDependency = true;
    if (
      /"vue-tsc"\s*:/u.test(raw) ||
      /(?:^|[^A-Za-z0-9_-])vue-tsc(?:$|[^A-Za-z0-9_-])/u.test(raw)
    ) {
      hasVueTsc = true;
    }
    if (/"@vue\/typescript-plugin"\s*:/u.test(raw)) hasVueTypeScriptPlugin = true;
    if (/"@vue\/language-tools"\s*:/u.test(raw)) hasVueLanguageTools = true;
    if (/"@vitejs\/plugin-vue"\s*:/u.test(raw)) hasViteVue = true;
    if (/"rollup-plugin-vue"\s*:/u.test(raw)) hasRollupVue = true;
    if (/"nuxt"\s*:/u.test(raw)) hasNuxt = true;

    if (pkg && typeof pkg.name === "string") {
      packages.push({
        path: relPath,
        name: pkg.name,
      });
    }
  }

  let workspaceMentionsVolar = false;
  let workspaceMentionsVuePlugin = false;
  const workspaceMatches = [];
  for (const workspaceFile of scan.workspaceFiles) {
    const raw = safeRead(workspaceFile);
    const relPath = normalizePath(path.relative(repoRoot, workspaceFile));
    const matches = [];
    if (/Vue Official|volar|Vue\.volar|Vue\.vscode-typescript-vue-plugin/u.test(raw)) {
      workspaceMentionsVolar = true;
      matches.push("volar");
    }
    if (/@vue\/typescript-plugin|vscode-typescript-vue-plugin/u.test(raw)) {
      workspaceMentionsVuePlugin = true;
      matches.push("@vue/typescript-plugin");
    }
    if (matches.length > 0) {
      workspaceMatches.push({ path: relPath, matches });
    }
  }

  let configMentionsViteVue = false;
  let configMentionsRollupVue = false;
  let configMentionsNuxt = false;
  for (const configFile of scan.configFiles) {
    const raw = safeRead(configFile);
    const relPath = normalizePath(path.relative(repoRoot, configFile));
    if (raw.includes("@vitejs/plugin-vue")) {
      configMentionsViteVue = true;
      buildSurfaceKinds.add("vite");
      buildSurfaceMatches.push({ bundler: "vite", path: relPath });
    }
    if (raw.includes("rollup-plugin-vue")) {
      configMentionsRollupVue = true;
      buildSurfaceKinds.add("rollup");
      buildSurfaceMatches.push({ bundler: "rollup", path: relPath });
    }
    if (path.basename(configFile).startsWith("nuxt.config") || raw.includes("defineNuxtConfig")) {
      configMentionsNuxt = true;
      buildSurfaceKinds.add("nuxt");
      buildSurfaceMatches.push({ bundler: "nuxt", path: relPath });
    }
  }

  if (/"@vitejs\/plugin-vue"\s*:/u.test(rootPackageRaw)) {
    buildSurfaceKinds.add("vite");
    buildSurfaceMatches.push({ bundler: "vite", path: "package.json" });
  }
  if (/"rollup-plugin-vue"\s*:/u.test(rootPackageRaw)) {
    buildSurfaceKinds.add("rollup");
    buildSurfaceMatches.push({ bundler: "rollup", path: "package.json" });
  }
  if (/"nuxt"\s*:/u.test(rootPackageRaw)) {
    buildSurfaceKinds.add("nuxt");
    buildSurfaceMatches.push({ bundler: "nuxt", path: "package.json" });
  }

  const buildKinds = [...buildSurfaceKinds];
  const buildBundler = buildKinds.length === 1 ? buildKinds[0] : null;
  const isToolchainRepo = isVerterToolchainName(rootPackage?.name);

  return {
    packages,
    workspaceMatches,
    hasVueDependency,
    hasVueTsc,
    hasVueTypeScriptPlugin,
    hasVueLanguageTools,
    hasViteVue: hasViteVue || configMentionsViteVue,
    hasRollupVue: hasRollupVue || configMentionsRollupVue,
    hasNuxt: hasNuxt || configMentionsNuxt,
    workspaceMentionsVolar,
    workspaceMentionsVuePlugin,
    buildSurfaceKinds: buildKinds,
    buildSurfaceMatches,
    buildBundler,
    isToolchainRepo,
  };
}

function chooseTsconfig(repoRoot) {
  const available = ROOT_TS_CONFIG_NAMES.filter((name) => fs.existsSync(path.join(repoRoot, name)));
  if (available.length === 0) return null;

  const rootTsconfig = path.join(repoRoot, "tsconfig.json");
  if (fs.existsSync(rootTsconfig)) {
    const raw = tryParseJson(safeRead(rootTsconfig));
    const hasFiles = Array.isArray(raw?.files) && raw.files.length > 0;
    const hasInclude = Array.isArray(raw?.include) && raw.include.length > 0;
    const hasRefs = Array.isArray(raw?.references) && raw.references.length > 0;
    if (!hasFiles && !hasInclude && hasRefs) {
      for (const alt of ROOT_TS_CONFIG_NAMES.slice(1)) {
        const altPath = path.join(repoRoot, alt);
        if (fs.existsSync(altPath)) {
          return normalizePath(path.relative(repoRoot, altPath));
        }
      }
    }
    return "tsconfig.json";
  }

  return normalizePath(path.relative(repoRoot, path.join(repoRoot, available[0])));
}

function pickCommands(rootPackageRaw) {
  const pkg = tryParseJson(rootPackageRaw);
  const scripts = pkg?.scripts && typeof pkg.scripts === "object" ? pkg.scripts : {};
  const buildCmd =
    typeof scripts.build === "string" && scripts.build.trim() ? scripts.build.trim() : null;
  let testCmd = null;
  if (typeof scripts.test === "string" && scripts.test.trim()) {
    testCmd = scripts.test.trim();
  } else if (typeof scripts["test:unit"] === "string" && scripts["test:unit"].trim()) {
    testCmd = scripts["test:unit"].trim();
  }
  return { buildCmd, testCmd };
}

function classifyRecipe({
  buildSurface,
  typecheckSurface,
  editorSurface,
  buildCmd,
  packageManager,
  chosenTsconfig,
  buildBundler,
  isToolchainRepo,
}) {
  if (isToolchainRepo) return "manual_review";
  const hasExecutableTypecheck =
    typecheckSurface && Boolean(packageManager) && Boolean(chosenTsconfig);
  const hasExecutableBuild =
    buildSurface && Boolean(buildBundler) && Boolean(packageManager) && Boolean(buildCmd);

  if (hasExecutableBuild && (hasExecutableTypecheck || editorSurface)) return "full_stack";
  if (hasExecutableTypecheck) return "typecheck_only";
  if (editorSurface) return "editor_only";
  if (hasExecutableBuild) return "build_only";
  return "manual_review";
}

function createReplacementSteps(surfaceFlags, recipe) {
  if (recipe === "manual_review") return [];

  const steps = [];
  if (surfaceFlags.workspaceMentionsVolar || surfaceFlags.workspaceMentionsVuePlugin) {
    steps.push("editor");
  }
  if (
    surfaceFlags.hasVueTypeScriptPlugin ||
    surfaceFlags.hasVueLanguageTools ||
    surfaceFlags.hasVueTsc
  ) {
    steps.push("typescript-plugin");
  }
  if (surfaceFlags.hasVueTsc) {
    steps.push("verter-tsc");
  }
  if (surfaceFlags.buildBundler === "nuxt") {
    steps.push("nuxt-module");
  } else if (surfaceFlags.buildBundler) {
    steps.push("build-plugin");
  }
  return [...new Set(steps)];
}

function buildTier1Entries(matrixProjects) {
  return matrixProjects.map((project) => ({
    id: `matrix:${project.name}`,
    name: project.name,
    source: "matrix",
    executionTier: "tier1",
    replacementRecipe: "full_stack",
    replacementSteps: [project.bundler === "nuxt" ? "nuxt-module" : "build-plugin", "verter-tsc"],
    repo: `https://github.com/${project.repo}`,
    packageManager: project.packageManager,
    buildBundler: project.bundler,
    buildCmd: project.buildCmd,
    testCmd: project.testCmd || null,
    chosenTsconfig: "tsconfig.json",
  }));
}

export function analyzeRepo(repoRoot, { discoveryRoot } = {}) {
  const rootPackagePath = path.join(repoRoot, "package.json");
  const rootPackageRaw = fs.existsSync(rootPackagePath) ? safeRead(rootPackagePath) : "";
  const scan = scanRepo(repoRoot);
  const surfaceFlags = extractSurfaceFlags(repoRoot, scan, rootPackageRaw);
  const chosenTsconfig = chooseTsconfig(repoRoot);
  const { packageManager, lockfile } = detectPackageManager(repoRoot, rootPackageRaw);
  const { buildCmd, testCmd } = pickCommands(rootPackageRaw);

  const buildSurface = surfaceFlags.buildSurfaceKinds.length > 0;
  const editorSurface =
    surfaceFlags.workspaceMentionsVolar || surfaceFlags.workspaceMentionsVuePlugin;
  const typecheckSurface =
    surfaceFlags.hasVueTsc ||
    surfaceFlags.hasVueTypeScriptPlugin ||
    surfaceFlags.hasVueLanguageTools ||
    (scan.vueFileCount > 0 && Boolean(chosenTsconfig));

  const recipe = classifyRecipe({
    buildSurface,
    typecheckSurface,
    editorSurface,
    buildCmd,
    packageManager,
    chosenTsconfig,
    buildBundler: surfaceFlags.buildBundler,
    isToolchainRepo: surfaceFlags.isToolchainRepo,
  });
  const replacementSteps = createReplacementSteps(surfaceFlags, recipe);

  const rawRelativeRoot = discoveryRoot
    ? normalizePath(path.relative(discoveryRoot, repoRoot))
    : normalizePath(repoRoot);
  const relativeRoot =
    rawRelativeRoot && rawRelativeRoot !== "." ? rawRelativeRoot : path.basename(repoRoot);
  const id =
    relativeRoot.replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "") ||
    path.basename(repoRoot);

  const reasons = [];
  if (recipe === "manual_review") {
    if (surfaceFlags.isToolchainRepo) reasons.push("repo is verter toolchain");
    if (surfaceFlags.buildSurfaceKinds.length > 1)
      reasons.push(`ambiguous build surface: ${surfaceFlags.buildSurfaceKinds.join(", ")}`);
    if (typecheckSurface && !chosenTsconfig) reasons.push("missing root tsconfig");
    if ((typecheckSurface || buildSurface) && !packageManager)
      reasons.push("missing package manager");
    if (buildSurface && !buildCmd) reasons.push("missing scripts.build");
  }

  return {
    id,
    name: path.basename(repoRoot),
    source: "local",
    repoRoot: normalizePath(repoRoot),
    relativeRoot,
    packageManager,
    lockfile,
    workspaceFiles: scan.workspaceFiles.map((file) => normalizePath(path.relative(repoRoot, file))),
    configFiles: scan.configFiles.map((file) => normalizePath(path.relative(repoRoot, file))),
    tsconfigFiles: scan.tsconfigFiles.map((file) => normalizePath(path.relative(repoRoot, file))),
    sampleVueFiles: scan.vueFiles.map((file) => normalizePath(path.relative(repoRoot, file))),
    vueFileCount: scan.vueFileCount,
    nestedPackages: surfaceFlags.packages,
    surfaces: {
      editor: editorSurface,
      typecheck: typecheckSurface,
      build: buildSurface,
      hasVueTsc: surfaceFlags.hasVueTsc,
      hasVueTypeScriptPlugin: surfaceFlags.hasVueTypeScriptPlugin,
      hasVueLanguageTools: surfaceFlags.hasVueLanguageTools,
      hasViteVue: surfaceFlags.hasViteVue,
      hasRollupVue: surfaceFlags.hasRollupVue,
      hasNuxt: surfaceFlags.hasNuxt,
      workspaceMentionsVolar: surfaceFlags.workspaceMentionsVolar,
      workspaceMentionsVuePlugin: surfaceFlags.workspaceMentionsVuePlugin,
      buildSurfaceKinds: surfaceFlags.buildSurfaceKinds,
      buildSurfaceMatches: surfaceFlags.buildSurfaceMatches,
      buildBundler: surfaceFlags.buildBundler,
      isToolchainRepo: surfaceFlags.isToolchainRepo,
    },
    workspaceMatches: surfaceFlags.workspaceMatches,
    chosenTsconfig,
    buildCmd,
    testCmd,
    executionTier: recipe === "manual_review" ? "manual_review" : "tier2",
    replacementRecipe: recipe,
    replacementSteps,
    reasons,
  };
}

export function buildDiscoveryInventory({
  roots = ["D:/dev"],
  repoFilter = null,
  matrixProjects = [],
} = {}) {
  const normalizedRoots = roots.map((root) => normalizePath(root));
  const filter = repoFilter ? new RegExp(repoFilter, "i") : null;

  const localRepos = [];
  for (const root of roots) {
    for (const repoRoot of findGitRepos(root, filter)) {
      const repo = analyzeRepo(repoRoot, { discoveryRoot: root });
      const isValid = repo.surfaces.editor || repo.surfaces.typecheck || repo.surfaces.build;
      if (isValid) localRepos.push(repo);
    }
  }

  localRepos.sort((a, b) => a.repoRoot.localeCompare(b.repoRoot));

  const tier1Projects = buildTier1Entries(matrixProjects);
  const summary = {
    tier1Count: tier1Projects.length,
    tier2Count: localRepos.length,
    byRecipe: countBy(localRepos, "replacementRecipe"),
    byExecutionTier: countBy(localRepos, "executionTier"),
    byPackageManager: countBy(localRepos, "packageManager"),
  };

  return {
    schema: DISCOVERY_SCHEMA,
    generatedAt: new Date().toISOString(),
    roots: normalizedRoots,
    tier1Projects,
    localProjects: localRepos,
    summary,
  };
}

export function renderDiscoveryMarkdown(inventory) {
  const lines = [];
  lines.push("# Verter Local Discovery");
  lines.push("");
  lines.push(`Generated: ${inventory.generatedAt}`);
  lines.push("");
  lines.push("## Summary");
  lines.push("");
  lines.push(`- Tier 1 projects: ${inventory.summary.tier1Count}`);
  lines.push(`- Tier 2 local repos: ${inventory.summary.tier2Count}`);
  lines.push("");
  lines.push("| Recipe | Count |");
  lines.push("|--------|------:|");
  for (const [recipe, count] of Object.entries(inventory.summary.byRecipe)) {
    lines.push(`| ${recipe} | ${count} |`);
  }
  lines.push("");
  lines.push("## Tier 1");
  lines.push("");
  lines.push("| Project | Bundler | PM | Build | Test |");
  lines.push("|---------|---------|----|-------|------|");
  for (const project of inventory.tier1Projects) {
    lines.push(
      `| ${project.name} | ${project.buildBundler} | ${project.packageManager} | ${project.buildCmd ?? "-"} | ${project.testCmd ?? "-"} |`,
    );
  }
  lines.push("");

  const grouped = inventory.localProjects.reduce((acc, project) => {
    const key = project.replacementRecipe;
    acc[key] = acc[key] || [];
    acc[key].push(project);
    return acc;
  }, {});

  for (const recipe of [
    "full_stack",
    "typecheck_only",
    "editor_only",
    "build_only",
    "manual_review",
  ]) {
    const items = grouped[recipe] || [];
    if (items.length === 0) continue;
    lines.push(`## ${recipe}`);
    lines.push("");
    lines.push("| Repo | Tier | Steps | PM | Bundler | Build | Test | Tsconfig | Reasons |");
    lines.push("|------|------|-------|----|---------|-------|------|----------|---------|");
    for (const project of items) {
      const steps = project.replacementSteps.join(", ") || "-";
      const reasons = project.reasons.join("; ") || "-";
      lines.push(
        `| ${project.relativeRoot} | ${project.executionTier} | ${steps} | ${project.packageManager ?? "-"} | ${project.surfaces.buildBundler ?? "-"} | ${project.buildCmd ?? "-"} | ${project.testCmd ?? "-"} | ${project.chosenTsconfig ?? "-"} | ${reasons} |`,
      );
    }
    lines.push("");
  }

  return lines.join("\n");
}
