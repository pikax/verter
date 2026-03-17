import type tsModule from "typescript/lib/tsserverlibrary";
import { SourceMap } from "node:module";
import type { VerterHost, Workspace } from "@verter/native";
import { hydrateMacroTypeDependencies, type MacroTypeDependencyAccess } from "./macroTypeHydration";
import type { VuePublicApiMode } from "./utils";
import { normalizePath, toVueVirtualFileName } from "./utils";

export const FALLBACK_STUB = "export default {} as any";

export interface CachedVirtualPublicApi {
  code: string;
  sourceMap: SourceMap | null;
}

let workspace: Workspace | null = null;
let host: VerterHost | null = null;
let loadFailed = false;
let loadError: string | null = null;
const virtualPublicApiCache = new Map<string, CachedVirtualPublicApi>();

function setCachedVirtualPublicApi(
  fileName: string,
  mode: VuePublicApiMode,
  code: string,
  rawSourceMap: string | undefined,
): void {
  const normalized = normalizePath(fileName);
  const parsedSourceMap = rawSourceMap ? tryCreateSourceMap(rawSourceMap) : null;
  const entry: CachedVirtualPublicApi = {
    code,
    sourceMap: parsedSourceMap,
  };

  if (mode === "testing") {
    virtualPublicApiCache.set(toVueVirtualFileName(normalized, "testing"), entry);
    return;
  }

  virtualPublicApiCache.set(toVueVirtualFileName(normalized, "public"), entry);
  virtualPublicApiCache.set(normalized + ".d.ts", entry);
}

function clearCachedVirtualPublicApi(fileName: string, mode?: VuePublicApiMode): void {
  const normalized = normalizePath(fileName);
  if (!mode || mode === "public") {
    virtualPublicApiCache.delete(toVueVirtualFileName(normalized, "public"));
    virtualPublicApiCache.delete(normalized + ".d.ts");
  }
  if (!mode || mode === "testing") {
    virtualPublicApiCache.delete(toVueVirtualFileName(normalized, "testing"));
  }
}

function tryCreateSourceMap(rawSourceMap: string): SourceMap | null {
  try {
    return new SourceMap(JSON.parse(rawSourceMap));
  } catch {
    return null;
  }
}

function offsetToLineColumn(text: string, offset: number): { line: number; column: number } {
  const prefix = text.slice(0, offset);
  const lines = prefix.split("\n");
  const lastLine = lines.length > 0 ? lines[lines.length - 1] : "";
  return {
    line: lines.length,
    column: lastLine.length + 1,
  };
}

function lineColumnToOffset(text: string, line: number, column: number): number | null {
  if (line < 1 || column < 1) return null;
  const lines = text.split("\n");
  if (line > lines.length) return null;

  let offset = 0;
  for (let i = 0; i < line - 1; i += 1) {
    offset += lines[i].length + 1;
  }
  const lineText = lines[line - 1];
  if (column - 1 > lineText.length) return null;
  return offset + column - 1;
}

export function getCachedVirtualPublicApi(fileName: string): CachedVirtualPublicApi | undefined {
  return virtualPublicApiCache.get(normalizePath(fileName));
}

export function clearVirtualPublicApiCache(): void {
  virtualPublicApiCache.clear();
}

export function remapVirtualSpan(
  fileName: string,
  span: { start: number; length: number },
  readOriginal: (fileName: string) => string | undefined,
): { fileName: string; textSpan: { start: number; length: number } } | null {
  const cached = getCachedVirtualPublicApi(fileName);
  if (!cached?.sourceMap) return null;

  const { line, column } = offsetToLineColumn(cached.code, span.start);
  const origin = cached.sourceMap.findOrigin(line, column);
  if (!("fileName" in origin) || !origin.fileName) {
    return null;
  }

  const originalFileName = normalizePath(origin.fileName);
  const originalText = readOriginal(originalFileName);
  if (!originalText) {
    return null;
  }

  const originalOffset = lineColumnToOffset(originalText, origin.lineNumber, origin.columnNumber);
  if (originalOffset == null) {
    return null;
  }

  return {
    fileName: originalFileName,
    textSpan: {
      start: originalOffset,
      length: 1,
    },
  };
}

function getHost(projectRoot: string): VerterHost | null {
  if (host) return host;
  if (loadFailed) return null;

  try {
    const native: typeof import("@verter/native") = require("@verter/native");
    if (!workspace) {
      workspace = new native.Workspace([projectRoot]);
    }
    host = native.VerterHost.withWorkspace({}, workspace);
    return host;
  } catch (e: unknown) {
    loadFailed = true;
    loadError = e instanceof Error ? e.message : String(e);
    return null;
  }
}

export const parseFile = (
  fileName: string,
  content: string,
  logger: tsModule.server.Logger,
  projectRoot: string,
  access?: MacroTypeDependencyAccess,
  mode: VuePublicApiMode = "public",
): string => {
  logger.info(`[Verter] parsing ${fileName}`);

  const h = getHost(projectRoot);
  if (!h) {
    clearCachedVirtualPublicApi(fileName, mode);
    logger.info(
      `[Verter] native binary not available, returning stub${loadError ? ` (error: ${loadError})` : ""}`,
    );
    return FALLBACK_STUB;
  }

  try {
    h.upsert({ inputId: fileName, source: content });
    hydrateMacroTypeDependencies(h, fileName, access);

    // getPublicApi() performs macro-only extraction (fast path — no full template compilation).
    // The generated code includes a //# sourceMappingURL= for Go-to-Definition support.
    const tsc = h.getPublicApi(fileName, mode);
    if (!tsc) {
      logger.info(`[Verter] getPublicApi returned null for ${fileName}, no script block`);
      clearCachedVirtualPublicApi(fileName, mode);
      return FALLBACK_STUB;
    }

    setCachedVirtualPublicApi(fileName, mode, tsc.code, tsc.sourceMap);
    logger.info(`[Verter] compiled ${fileName} (${tsc.code.length} chars)`);
    return tsc.code;
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    logger.info(`[Verter] compilation error for ${fileName}: ${msg}`);
    clearCachedVirtualPublicApi(fileName, mode);
    return FALLBACK_STUB;
  }
};
