import { readdirSync, readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { join, relative, resolve } from "node:path";

import { loadVerterCompatModule } from "./verter-compat.js";

const componentName = process.argv[2];
const componentAliases: Record<string, string> = {
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

if (!componentName) {
  console.error("Usage: tsx src/_trace-component.ts <ComponentName>");
  process.exit(1);
}

function maybeGc(): void {
  (globalThis as typeof globalThis & { gc?: () => void }).gc?.();
}

function formatMemoryUsage(): string {
  const usage = process.memoryUsage();
  const heapMb = Math.round(usage.heapUsed / 1024 / 1024);
  const rssMb = Math.round(usage.rss / 1024 / 1024);
  return `heap=${heapMb}MB rss=${rssMb}MB`;
}

const uiRoot = resolve("../../.integration-tests/repos/nuxt-ui");
const componentsRoot = resolve(uiRoot, "src/runtime/components");

function listVueFiles(root: string): string[] {
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

function componentScore(token: string, filePath: string, source: string): number {
  let score = 0;
  const rel = relative(componentsRoot, filePath).replace(/\\/g, "/");
  if (!rel.includes("/")) score += 100;
  if (!rel.startsWith("prose/")) score += 50;
  if (source.includes(`'${token}'`) || source.includes(`"${token}"`)) score += 30;
  if (source.includes(token)) score += 20;
  score -= rel.length;
  return score;
}

function resolveComponentFile(token: string): string {
  const direct = resolve(componentsRoot, `${token}.vue`);
  try {
    readFileSync(direct, "utf-8");
    return direct;
  } catch {}

  const alias = componentAliases[token];
  if (alias) {
    return resolveComponentFile(alias);
  }

  const matches = listVueFiles(componentsRoot)
    .map((filePath) => ({ filePath, source: readFileSync(filePath, "utf-8") }))
    .filter(({ source }) => source.includes(token))
    .sort(
      (a, b) =>
        componentScore(token, b.filePath, b.source) - componentScore(token, a.filePath, a.source),
    );

  if (matches.length === 0) {
    throw new Error(`Unable to resolve component token ${token}`);
  }

  return matches[0].filePath;
}

const file = resolveComponentFile(componentName).replace(/\\/g, "/");

const source = readFileSync(file.replace(/\//g, "\\"), "utf-8");
maybeGc();
const heapBeforeSetup = formatMemoryUsage();
const setupStart = performance.now();
const compat = await loadVerterCompatModule();
const checker = await compat.createCheckerByJson(
  uiRoot.replace(/\\/g, "/"),
  {
    compilerOptions: { strict: true, jsx: "preserve" },
  },
  {
    forceUseTs: true,
    runtimeMode: "dedicated",
    typeExpansionBackend: "verter",
  },
);
const setupMs = Math.round(performance.now() - setupStart);
maybeGc();
const heapAfterSetup = formatMemoryUsage();

try {
  checker.updateFile(file, source);
  maybeGc();
  const heapBeforeQuery = formatMemoryUsage();
  const start = performance.now();
  const meta = await checker.getComponentMeta(file);
  const durationMs = Math.round(performance.now() - start);
  maybeGc();
  const heapAfterQuery = formatMemoryUsage();
  console.log(
    `Done in ${durationMs}ms (${meta?.props?.length ?? 0} props) setup=${setupMs}ms setup ${heapBeforeSetup}->${heapAfterSetup} query ${heapBeforeQuery}->${heapAfterQuery}`,
  );
} finally {
  checker.close();
  maybeGc();
  console.log(`Closed ${formatMemoryUsage()}`);
}
