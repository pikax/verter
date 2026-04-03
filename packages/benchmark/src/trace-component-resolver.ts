import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";

export const componentAliases: Record<string, string> = {
  DatePicker: "InputDate",
  Dialog: "Modal",
  HoverCardContent: "Popover",
  Menubar: "DropdownMenu",
  MenubarContent: "DropdownMenuContent",
  Sheet: "Slideover",
  SheetContent: "Slideover",
  SlideoverContent: "Slideover",
  StepperContent: "Stepper",
};

export type ResolveComponentFileOptions = {
  componentsRoot?: string;
  registry?: Map<string, string>;
  uiRoot?: string;
  vueFiles?: string[];
};

const GENERATED_COMPONENT_PATTERN =
  /^export const (\w+): typeof import\(["'](\.\.\/src\/runtime\/components\/[^"']+\.vue)["']\)\['default'\]/gm;

export function getDefaultUiRoot(fromDir = import.meta.dirname): string {
  return resolve(fromDir, "../../../.integration-tests/repos/nuxt-ui");
}

export function getComponentsRoot(uiRoot: string): string {
  return resolve(uiRoot, "src/runtime/components");
}

export function loadGeneratedComponentRegistry(uiRoot: string): Map<string, string> {
  const declarationsPath = resolve(uiRoot, ".nuxt/components.d.ts");
  const declarationsDir = resolve(uiRoot, ".nuxt");
  const declarations = readFileSync(declarationsPath, "utf-8");
  const registry = new Map<string, string>();

  for (const match of declarations.matchAll(GENERATED_COMPONENT_PATTERN)) {
    const componentName = match[1];
    const relativeImportPath = match[2];
    if (!componentName || !relativeImportPath) {
      continue;
    }
    registry.set(componentName, resolve(declarationsDir, relativeImportPath));
  }

  return registry;
}

export function listVueFiles(root: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const entryPath = join(root, entry.name);
    if (entry.isDirectory()) {
      files.push(...listVueFiles(entryPath));
      continue;
    }
    if (entry.isFile() && entry.name.endsWith(".vue")) {
      files.push(entryPath);
    }
  }
  return files;
}

function componentScore(
  token: string,
  componentsRoot: string,
  filePath: string,
  source: string,
): number {
  let score = 0;
  const rel = relative(componentsRoot, filePath).replace(/\\/g, "/");
  if (!rel.includes("/")) score += 100;
  if (!rel.startsWith("prose/")) score += 50;
  if (source.includes(`'${token}'`) || source.includes(`"${token}"`)) score += 30;
  if (source.includes(token)) score += 20;
  score -= rel.length;
  return score;
}

function stripGeneratedPrefix(token: string): string {
  if (/^Prose[A-Z]/.test(token)) {
    return token.slice("Prose".length);
  }
  if (/^U[A-Z]/.test(token)) {
    return token.slice(1);
  }
  return token;
}

function buildRegistryCandidates(token: string): string[] {
  const normalized = stripGeneratedPrefix(token);
  return [...new Set([token, normalized, `U${normalized}`, `Prose${normalized}`])];
}

function resolveDirectFile(candidates: string[], componentsRoot: string): string | null {
  for (const candidate of candidates) {
    const directPath = resolve(componentsRoot, `${candidate}.vue`);
    if (existsSync(directPath)) {
      return directPath;
    }
  }
  return null;
}

function resolveViaRegistry(candidates: string[], registry: Map<string, string>): string | null {
  for (const candidate of candidates) {
    const resolved = registry.get(candidate);
    if (resolved && existsSync(resolved)) {
      return resolved;
    }
  }
  return null;
}

function resolveViaSourceScan(
  token: string,
  candidates: string[],
  componentsRoot: string,
  vueFiles: string[],
): string | null {
  const matches = vueFiles
    .map((filePath) => ({ filePath, source: readFileSync(filePath, "utf-8") }))
    .filter(({ source }) => candidates.some((candidate) => source.includes(candidate)))
    .sort(
      (a, b) =>
        componentScore(token, componentsRoot, b.filePath, b.source) -
        componentScore(token, componentsRoot, a.filePath, a.source),
    );

  return matches[0]?.filePath ?? null;
}

function resolveComponentFileInner(
  token: string,
  options: Required<ResolveComponentFileOptions>,
  seen: Set<string>,
): string {
  if (seen.has(token)) {
    throw new Error(`Recursive component alias detected for "${token}"`);
  }
  seen.add(token);

  const candidates = buildRegistryCandidates(token);

  const direct = resolveDirectFile(candidates, options.componentsRoot);
  if (direct) {
    return direct;
  }

  const registryResolved = resolveViaRegistry(candidates, options.registry);
  if (registryResolved) {
    return registryResolved;
  }

  const alias = componentAliases[token] ?? componentAliases[stripGeneratedPrefix(token)];
  if (alias) {
    return resolveComponentFileInner(alias, options, seen);
  }

  const sourceResolved = resolveViaSourceScan(
    token,
    candidates,
    options.componentsRoot,
    options.vueFiles,
  );
  if (sourceResolved) {
    return sourceResolved;
  }

  throw new Error(
    `Unable to resolve component "${token}" in ${options.componentsRoot}; tried ${candidates.join(", ")}`,
  );
}

export function resolveComponentFile(
  token: string,
  options: ResolveComponentFileOptions = {},
): string {
  const uiRoot = options.uiRoot ?? getDefaultUiRoot(import.meta.dirname);
  const componentsRoot = options.componentsRoot ?? getComponentsRoot(uiRoot);
  const registry = options.registry ?? loadGeneratedComponentRegistry(uiRoot);
  const vueFiles = options.vueFiles ?? listVueFiles(componentsRoot);

  return resolveComponentFileInner(
    token,
    { componentsRoot, registry, uiRoot, vueFiles },
    new Set(),
  );
}
