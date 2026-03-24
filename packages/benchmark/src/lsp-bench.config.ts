import { existsSync } from "node:fs";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { createRequire } from "node:module";

const DEFAULT_WORKSPACE_REL = "packages/example";
const DEFAULT_TEST_FILE_REL = "Test.vue";
const DEFAULT_HOVER_LINE = 2;
const DEFAULT_HOVER_CHAR = 9;
const moduleRequire = createRequire(import.meta.url);

export interface BenchmarkTarget {
  workspaceRoot: string;
  testFile: string;
  testFileRel: string;
  hoverLine: number;
  hoverChar: number;
}

export interface LspBenchConfig extends BenchmarkTarget {
  jsonMode: boolean;
  skipVolar: boolean;
  verterBin: string;
  volarScript?: string;
  tsdkPath?: string;
  projectName: string;
}

interface CommonResolveOptions {
  cwd?: string;
  pathExists?: (path: string) => boolean;
}

interface PackageResolveOptions {
  resolvePackage?: (specifier: string) => string;
}

interface ParseConfigOptions extends CommonResolveOptions, PackageResolveOptions {
  argv: string[];
  env: NodeJS.ProcessEnv;
  platform: NodeJS.Platform;
  repoRoot: string;
}

interface ResolveVerterBinaryOptions extends CommonResolveOptions {
  repoRoot: string;
  override?: string;
  platform?: NodeJS.Platform;
}

interface ResolveTypeScriptSdkOptions extends CommonResolveOptions, PackageResolveOptions {
  workspaceRoot: string;
  repoRoot: string;
  override?: string;
}

interface ResolveVolarScriptOptions extends CommonResolveOptions, PackageResolveOptions {
  override?: string;
}

interface ResolveBenchmarkTargetOptions extends CommonResolveOptions {
  repoRoot: string;
  workspace?: string;
  file?: string;
  hoverLine?: string;
  hoverChar?: string;
}

function getFlag(argv: string[], name: string): string | undefined {
  const prefix = `--${name}=`;
  const arg = argv.find((entry) => entry.startsWith(prefix));
  return arg ? arg.slice(prefix.length) : undefined;
}

function resolveInputPath(input: string, cwd: string): string {
  return isAbsolute(input) ? input : resolve(cwd, input);
}

function normalizeSlashes(path: string): string {
  return path.replaceAll("\\", "/");
}

function parseOneBasedNumber(
  value: string | undefined,
  flagName: string,
  fallback: number,
): number {
  if (value == null || value === "") {
    return fallback;
  }

  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed < 1) {
    throw new Error(`Invalid --${flagName} value '${value}'. Expected a 1-based integer.`);
  }
  return parsed;
}

function defaultPathExists(path: string): boolean {
  return existsSync(path);
}

function defaultPackageResolver(): (specifier: string) => string {
  return (specifier: string) => moduleRequire.resolve(specifier);
}

export function resolveVerterBinary(options: ResolveVerterBinaryOptions): string {
  const pathExists = options.pathExists ?? defaultPathExists;
  const cwd = options.cwd ?? options.repoRoot;
  const platform = options.platform ?? process.platform;
  const binaryName = platform === "win32" ? "verter-lsp.exe" : "verter-lsp";

  const candidates = (
    options.override
      ? [resolveInputPath(options.override, cwd)]
      : [
          join(options.repoRoot, "target/release", binaryName),
          join(options.repoRoot, "target/debug", binaryName),
        ]
  ).map(normalizeSlashes);

  for (const candidate of candidates) {
    if (pathExists(candidate)) {
      return candidate;
    }
  }

  throw new Error(
    `Could not find the Verter LSP binary. Tried: ${candidates
      .map((candidate) => normalizeSlashes(candidate))
      .join(", ")}. Build it with 'pnpm run build:lsp' or pass --verter-bin=/path/to/verter-lsp.`,
  );
}

export function resolveTypeScriptSdk(options: ResolveTypeScriptSdkOptions): string {
  const pathExists = options.pathExists ?? defaultPathExists;
  const cwd = options.cwd ?? options.repoRoot;

  if (options.override) {
    const resolved = normalizeSlashes(resolveInputPath(options.override, cwd));
    if (!pathExists(resolved)) {
      throw new Error(`TypeScript SDK path does not exist: ${resolved}`);
    }
    return resolved;
  }

  const workspaceSdk = normalizeSlashes(join(options.workspaceRoot, "node_modules/typescript/lib"));
  if (pathExists(workspaceSdk)) {
    return workspaceSdk;
  }

  const repoSdk = normalizeSlashes(join(options.repoRoot, "node_modules/typescript/lib"));
  if (pathExists(repoSdk)) {
    return repoSdk;
  }

  const resolvePackage = options.resolvePackage ?? defaultPackageResolver();
  try {
    const packageJson = resolvePackage("typescript/package.json");
    const packageSdk = normalizeSlashes(join(dirname(packageJson), "lib"));
    if (!pathExists(packageSdk)) {
      throw new Error(`Resolved package SDK path does not exist: ${packageSdk}`);
    }
    return packageSdk;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(
      `Could not locate a TypeScript SDK for Volar. Checked workspace and repo node_modules. Install 'typescript' or pass --tsdk=/path/to/typescript/lib. (${message})`,
    );
  }
}

export function resolveVolarScript(options: ResolveVolarScriptOptions): string {
  const cwd = options.cwd ?? process.cwd();
  const pathExists = options.pathExists ?? defaultPathExists;
  if (options.override) {
    const resolved = normalizeSlashes(resolveInputPath(options.override, cwd));
    if (!pathExists(resolved)) {
      throw new Error(`Volar script path does not exist: ${resolved}`);
    }
    return resolved;
  }

  const resolvePackage = options.resolvePackage ?? defaultPackageResolver();
  try {
    const packageJson = resolvePackage("@vue/language-server/package.json");
    const scriptPath = normalizeSlashes(join(dirname(packageJson), "bin/vue-language-server.js"));
    if (!pathExists(scriptPath)) {
      throw new Error(`Resolved Volar script path does not exist: ${scriptPath}`);
    }
    return scriptPath;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(
      `Could not locate Volar's language server. Install '@vue/language-server' or pass --volar-script=/path/to/vue-language-server.js. (${message})`,
    );
  }
}

export function resolveBenchmarkTarget(options: ResolveBenchmarkTargetOptions): BenchmarkTarget {
  const pathExists = options.pathExists ?? defaultPathExists;
  const cwd = options.cwd ?? options.repoRoot;
  const workspaceRoot = normalizeSlashes(
    options.workspace
      ? resolveInputPath(options.workspace, cwd)
      : join(options.repoRoot, DEFAULT_WORKSPACE_REL),
  );

  if (!pathExists(workspaceRoot)) {
    throw new Error(
      `Workspace path does not exist: ${workspaceRoot}. Pass --workspace=/path/to/vue-project.`,
    );
  }

  const testFile = normalizeSlashes(
    options.file
      ? resolveInputPath(options.file, workspaceRoot)
      : join(workspaceRoot, DEFAULT_TEST_FILE_REL),
  );
  if (!pathExists(testFile)) {
    throw new Error(
      `Benchmark test file does not exist: ${testFile}. Pass --file=relative/path/to/file.vue.`,
    );
  }

  const hoverLine = parseOneBasedNumber(options.hoverLine, "hover-line", DEFAULT_HOVER_LINE) - 1;
  const hoverChar = parseOneBasedNumber(options.hoverChar, "hover-char", DEFAULT_HOVER_CHAR) - 1;
  const relativeFile = normalizeSlashes(relative(workspaceRoot, testFile));

  return {
    workspaceRoot,
    testFile,
    testFileRel: relativeFile,
    hoverLine,
    hoverChar,
  };
}

export function parseLspBenchConfig(options: ParseConfigOptions): LspBenchConfig {
  const jsonMode = options.argv.includes("--json");
  const skipVolar = options.argv.includes("--skip-volar");

  const target = resolveBenchmarkTarget({
    repoRoot: options.repoRoot,
    cwd: options.cwd,
    pathExists: options.pathExists,
    workspace: getFlag(options.argv, "workspace") ?? options.env.LSP_BENCH_WORKSPACE,
    file: getFlag(options.argv, "file") ?? options.env.LSP_BENCH_FILE,
    hoverLine: getFlag(options.argv, "hover-line") ?? options.env.LSP_BENCH_HOVER_LINE,
    hoverChar: getFlag(options.argv, "hover-char") ?? options.env.LSP_BENCH_HOVER_CHAR,
  });

  const verterBin = resolveVerterBinary({
    repoRoot: options.repoRoot,
    cwd: options.cwd,
    override: getFlag(options.argv, "verter-bin") ?? options.env.LSP_BENCH_VERTER_BIN,
    platform: options.platform,
    pathExists: options.pathExists,
  });

  const config: LspBenchConfig = {
    ...target,
    jsonMode,
    skipVolar,
    verterBin,
    projectName: basename(target.workspaceRoot),
  };

  if (!skipVolar) {
    config.volarScript = resolveVolarScript({
      cwd: options.cwd,
      override: getFlag(options.argv, "volar-script") ?? options.env.LSP_BENCH_VOLAR_SCRIPT,
      resolvePackage: options.resolvePackage,
    });
    config.tsdkPath = resolveTypeScriptSdk({
      workspaceRoot: target.workspaceRoot,
      repoRoot: options.repoRoot,
      cwd: options.cwd,
      override: getFlag(options.argv, "tsdk") ?? options.env.LSP_BENCH_TSDK,
      pathExists: options.pathExists,
      resolvePackage: options.resolvePackage,
    });
  }

  return config;
}
