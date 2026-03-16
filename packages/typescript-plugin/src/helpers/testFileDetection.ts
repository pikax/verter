import path from "node:path";
import { isLikelyTestFileName, normalizePath } from "./utils";

const CONFIG_DISCOVERY_BUDGET_MS = 25;
const MAX_CONFIG_BYTES = 256 * 1024;
const RUNNER_CONFIG_FILES = [
  "vitest.config.ts",
  "vitest.config.mts",
  "vitest.config.cts",
  "vitest.config.js",
  "vitest.config.mjs",
  "vitest.config.cjs",
  "vite.config.ts",
  "vite.config.mts",
  "vite.config.cts",
  "vite.config.js",
  "vite.config.mjs",
  "vite.config.cjs",
  "jest.config.ts",
  "jest.config.mts",
  "jest.config.cts",
  "jest.config.js",
  "jest.config.mjs",
  "jest.config.cjs",
  "package.json",
] as const;

export interface TestFileDetectionHost {
  fileExists(fileName: string): boolean;
  readFile(fileName: string): string | undefined;
}

interface TestFileMatcher {
  rootDir: string;
  globs: RegExp[];
  regexes: RegExp[];
}

type ConfigLoadResult =
  | { kind: "ignore" }
  | { kind: "resolved"; matcher: TestFileMatcher | null };

const nearestMatcherCache = new Map<string, TestFileMatcher | null>();
const parsedConfigCache = new Map<string, ConfigLoadResult>();

export function clearTestFileDetectionCache(): void {
  nearestMatcherCache.clear();
  parsedConfigCache.clear();
}

export function isTestFileWithContext(
  fileName: string,
  host: TestFileDetectionHost,
): boolean {
  const normalizedFileName = normalizePath(fileName);
  if (isLikelyTestFileName(normalizedFileName)) {
    return true;
  }

  const matcher = findNearestMatcher(normalizedFileName, host);
  return matcher ? matcherMatchesFile(matcher, normalizedFileName) : false;
}

function findNearestMatcher(
  fileName: string,
  host: TestFileDetectionHost,
): TestFileMatcher | null {
  const startDir = path.posix.dirname(normalizePath(fileName));
  const cached = nearestMatcherCache.get(startDir);
  if (cached !== undefined) {
    return cached;
  }

  const deadline = Date.now() + CONFIG_DISCOVERY_BUDGET_MS;
  const visitedDirs: string[] = [];
  let currentDir = startDir;
  let resolved: TestFileMatcher | null = null;

  while (true) {
    visitedDirs.push(currentDir);

    const result = loadMatcherForDirectory(currentDir, host, deadline);
    if (result.kind === "resolved") {
      resolved = result.matcher;
      break;
    }

    const parentDir = path.posix.dirname(currentDir);
    if (parentDir === currentDir) {
      break;
    }
    currentDir = parentDir;
  }

  for (const dir of visitedDirs) {
    nearestMatcherCache.set(dir, resolved);
  }

  return resolved;
}

function loadMatcherForDirectory(
  directory: string,
  host: TestFileDetectionHost,
  deadline: number,
): ConfigLoadResult {
  for (const fileName of RUNNER_CONFIG_FILES) {
    const configPath = normalizePath(path.posix.join(directory, fileName));
    if (!host.fileExists(configPath)) {
      continue;
    }

    const cached = parsedConfigCache.get(configPath);
    if (cached) {
      return cached;
    }

    const result = parseConfigFile(configPath, host, deadline);
    parsedConfigCache.set(configPath, result);
    if (result.kind === "resolved") {
      return result;
    }
  }

  return { kind: "ignore" };
}

function parseConfigFile(
  configPath: string,
  host: TestFileDetectionHost,
  deadline: number,
): ConfigLoadResult {
  if (Date.now() > deadline) {
    return { kind: "resolved", matcher: null };
  }

  const rawConfig = host.readFile(configPath);
  if (!rawConfig || rawConfig.length > MAX_CONFIG_BYTES) {
    return { kind: "resolved", matcher: null };
  }

  if (configPath.endsWith("/package.json")) {
    return parsePackageJsonConfig(configPath, rawConfig);
  }

  const baseName = path.posix.basename(configPath);
  if (baseName.startsWith("jest.config.")) {
    return {
      kind: "resolved",
      matcher: buildMatcher(path.posix.dirname(configPath), {
        testMatch: extractStringArrayProperty(rawConfig, "testMatch"),
        testRegex: extractRegexProperty(rawConfig, "testRegex"),
      }),
    };
  }

  if (baseName.startsWith("vitest.config.")) {
    return {
      kind: "resolved",
      matcher: buildMatcher(path.posix.dirname(configPath), {
        include: extractStringArrayProperty(rawConfig, "include"),
      }),
    };
  }

  if (baseName.startsWith("vite.config.")) {
    const testBlock = extractObjectProperty(rawConfig, "test");
    if (!testBlock) {
      return { kind: "ignore" };
    }

    return {
      kind: "resolved",
      matcher: buildMatcher(path.posix.dirname(configPath), {
        include: extractStringArrayProperty(testBlock, "include"),
      }),
    };
  }

  return { kind: "ignore" };
}

function parsePackageJsonConfig(configPath: string, rawConfig: string): ConfigLoadResult {
  try {
    const parsed = JSON.parse(rawConfig) as {
      vitest?: { include?: string[] };
      jest?: { testMatch?: string[]; testRegex?: string | string[] };
    };

    const rootDir = path.posix.dirname(configPath);
    if (parsed.vitest?.include?.length) {
      return {
        kind: "resolved",
        matcher: buildMatcher(rootDir, { include: parsed.vitest.include }),
      };
    }

    if (parsed.jest?.testMatch?.length || parsed.jest?.testRegex) {
      return {
        kind: "resolved",
        matcher: buildMatcher(rootDir, {
          testMatch: parsed.jest?.testMatch,
          testRegex: parsed.jest?.testRegex,
        }),
      };
    }

    return { kind: "ignore" };
  } catch {
    return { kind: "resolved", matcher: null };
  }
}

function buildMatcher(
  rootDir: string,
  config: {
    include?: string[];
    testMatch?: string[];
    testRegex?: string | string[] | null;
  },
): TestFileMatcher | null {
  const globs = [
    ...(config.include ?? []),
    ...(config.testMatch ?? []),
  ]
    .map((pattern) => normalizeTestPattern(pattern))
    .flatMap((pattern) => {
      const compiled = globToRegExp(pattern);
      return compiled ? [compiled] : [];
    });

  const regexes = toArray(config.testRegex)
    .map((pattern) => parseRegexLike(pattern))
    .flatMap((pattern) => {
      return pattern ? [pattern] : [];
    });

  if (globs.length === 0 && regexes.length === 0) {
    return null;
  }

  return {
    rootDir: normalizePath(rootDir),
    globs,
    regexes,
  };
}

function matcherMatchesFile(matcher: TestFileMatcher, fileName: string): boolean {
  const normalizedFileName = normalizePath(fileName);
  const relativeFileName = normalizePath(
    path.posix.relative(matcher.rootDir, normalizedFileName),
  );

  if (relativeFileName.startsWith("../")) {
    return false;
  }

  return (
    matcher.globs.some(
      (regexp) =>
        regexp.test(relativeFileName) || regexp.test(`${matcher.rootDir}/${relativeFileName}`),
    ) || matcher.regexes.some((regexp) => regexp.test(normalizedFileName))
  );
}

function normalizeTestPattern(pattern: string): string {
  return normalizePath(pattern.trim())
    .replace(/^<rootDir>\//, "")
    .replace(/^\.\//, "");
}

function extractStringArrayProperty(sourceText: string, propertyName: string): string[] {
  const match = new RegExp(`\\b${propertyName}\\s*:\\s*\\[([\\s\\S]*?)\\]`, "m").exec(
    sourceText,
  );
  if (!match) {
    return [];
  }

  const values: string[] = [];
  const valuePattern = /["'`]([^"'`]+)["'`]/g;
  for (const value of match[1].matchAll(valuePattern)) {
    values.push(value[1]);
  }
  return values;
}

function extractRegexProperty(sourceText: string, propertyName: string): string[] {
  const arrayValues = extractStringArrayProperty(sourceText, propertyName);
  if (arrayValues.length > 0) {
    return arrayValues;
  }

  const match = new RegExp(
    `\\b${propertyName}\\s*:\\s*(/[^\\n]+/[a-z]*|["'\`][^"'\`]+["'\`])`,
    "m",
  ).exec(sourceText);
  return match ? [stripQuotes(match[1])] : [];
}

function extractObjectProperty(sourceText: string, propertyName: string): string | null {
  const match = new RegExp(`\\b${propertyName}\\s*:\\s*\\{`, "m").exec(sourceText);
  if (!match) {
    return null;
  }

  let depth = 0;
  let index = match.index + match[0].length - 1;
  let bodyStart = index + 1;
  let quote: string | null = null;

  while (index < sourceText.length) {
    const char = sourceText[index];
    const prev = index > 0 ? sourceText[index - 1] : "";

    if (quote) {
      if (char === quote && prev !== "\\") {
        quote = null;
      }
      index += 1;
      continue;
    }

    if (char === "'" || char === '"' || char === "`") {
      quote = char;
      index += 1;
      continue;
    }

    if (char === "{") {
      depth += 1;
    } else if (char === "}") {
      depth -= 1;
      if (depth === 0) {
        return sourceText.slice(bodyStart, index);
      }
    }

    index += 1;
  }

  return null;
}

function globToRegExp(pattern: string): RegExp | null {
  if (!pattern) {
    return null;
  }

  let regexpSource = "^";

  for (let index = 0; index < pattern.length; index += 1) {
    const char = pattern[index];
    const next = pattern[index + 1];

    if (char === "*" && next === "*") {
      const nextNext = pattern[index + 2];
      if (nextNext === "/") {
        regexpSource += "(?:.*/)?";
        index += 2;
      } else {
        regexpSource += ".*";
        index += 1;
      }
      continue;
    }

    if (char === "*") {
      regexpSource += "[^/]*";
      continue;
    }

    if (char === "?") {
      regexpSource += "[^/]";
      continue;
    }

    if (char === "{") {
      const closing = pattern.indexOf("}", index);
      if (closing > index) {
        const variants = pattern
          .slice(index + 1, closing)
          .split(",")
          .map((variant) => escapeForRegExp(variant));
        regexpSource += `(?:${variants.join("|")})`;
        index = closing;
        continue;
      }
    }

    regexpSource += escapeForRegExp(char);
  }

  regexpSource += "$";
  return new RegExp(regexpSource);
}

function parseRegexLike(pattern: string): RegExp | null {
  const normalizedPattern = stripQuotes(pattern.trim());
  if (!normalizedPattern) {
    return null;
  }

  if (normalizedPattern.startsWith("/") && normalizedPattern.lastIndexOf("/") > 0) {
    const lastSlash = normalizedPattern.lastIndexOf("/");
    const body = normalizedPattern.slice(1, lastSlash);
    const flags = normalizedPattern.slice(lastSlash + 1);
    try {
      return new RegExp(body, flags);
    } catch {
      return null;
    }
  }

  try {
    return new RegExp(normalizedPattern);
  } catch {
    return null;
  }
}

function stripQuotes(value: string): string {
  if (
    (value.startsWith('"') && value.endsWith('"')) ||
    (value.startsWith("'") && value.endsWith("'")) ||
    (value.startsWith("`") && value.endsWith("`"))
  ) {
    return value.slice(1, -1);
  }
  return value;
}

function escapeForRegExp(value: string): string {
  return value.replace(/[|\\{}()[\]^$+*?.]/g, "\\$&");
}

function toArray<T>(value: T | T[] | null | undefined): T[] {
  if (value == null) {
    return [];
  }
  return Array.isArray(value) ? value : [value];
}
