/**
 * Transpile Sass/Stylus to CSS with source maps.
 *
 * Uses project-installed compilers (detected by preprocessorResolver).
 * Caches results per document version.
 */

import type { RawSourceMap } from "source-map";
import type { PreprocessorCache } from "./preprocessorResolver";

export interface TranspileResult {
  css: string;
  sourceMap: RawSourceMap | null;
  /** Compilation errors, if any. */
  errors: Array<{ message: string; line?: number; column?: number }>;
}

/**
 * Transpile preprocessor source to CSS.
 *
 * @param content - The preprocessor source code (from virtual file).
 * @param lang - "sass" or "stylus".
 * @param filePath - URI of the parent Vue file (used for @import resolution).
 * @param cache - Preprocessor module cache.
 * @returns Transpiled CSS + source map, or null if compiler not available.
 */
export async function transpile(
  content: string,
  lang: "sass" | "stylus",
  filePath: string,
  cache: PreprocessorCache,
): Promise<TranspileResult | null> {
  if (lang === "sass") {
    return transpileSass(content, filePath, cache);
  }
  if (lang === "stylus") {
    return transpileStylus(content, filePath, cache);
  }
  return null;
}

function transpileSass(
  content: string,
  filePath: string,
  cache: PreprocessorCache,
): TranspileResult | null {
  const sass = cache.sass as SassModule | undefined;
  if (!sass) return null;

  try {
    const result = sass.compileString(content, {
      syntax: "indented" as const,
      sourceMap: true,
      url: filePathToUrl(filePath),
    });

    return {
      css: result.css,
      sourceMap: result.sourceMap ?? null,
      errors: [],
    };
  } catch (err: unknown) {
    const error = err as SassError;
    return {
      css: "",
      sourceMap: null,
      errors: [
        {
          message: error.message ?? String(err),
          line: error.span?.start?.line,
          column: error.span?.start?.column,
        },
      ],
    };
  }
}

function transpileStylus(
  content: string,
  filePath: string,
  cache: PreprocessorCache,
): TranspileResult | null {
  const stylusModule = cache.stylus as StylusModule | undefined;
  if (!stylusModule) return null;

  try {
    const renderer = stylusModule(content)
      .set("filename", filePathToFsPath(filePath))
      .set("sourcemap", { comment: false, basePath: "." });

    const css = renderer.render();
    const sourceMap = renderer.sourcemap as RawSourceMap | undefined;

    return {
      css,
      sourceMap: sourceMap ?? null,
      errors: [],
    };
  } catch (err: unknown) {
    const error = err as StylusError;
    return {
      css: "",
      sourceMap: null,
      errors: [
        {
          message: error.message ?? String(err),
          line: error.lineno,
          column: error.column,
        },
      ],
    };
  }
}

// ── Utility helpers ──────────────────────────────────────────────

function filePathToUrl(filePath: string): URL {
  // Handle file:// URIs
  if (filePath.startsWith("file://")) {
    return new URL(filePath);
  }
  // Handle Windows paths
  const normalized = filePath.replace(/\\/g, "/");
  return new URL(`file:///${normalized.replace(/^\//, "")}`);
}

function filePathToFsPath(filePath: string): string {
  if (filePath.startsWith("file://")) {
    try {
      const url = new URL(filePath);
      return url.pathname.replace(/^\/([A-Za-z]:)/, "$1");
    } catch {
      return filePath;
    }
  }
  return filePath;
}

// ── Type stubs for dynamically loaded modules ────────────────────

interface SassModule {
  compileString(
    source: string,
    options: {
      syntax: "indented" | "scss";
      sourceMap: boolean;
      url?: URL;
    },
  ): { css: string; sourceMap?: RawSourceMap };
}

interface SassError {
  message: string;
  span?: { start?: { line?: number; column?: number } };
}

interface StylusModule {
  (source: string): StylusRenderer;
}

interface StylusRenderer {
  set(key: string, value: unknown): StylusRenderer;
  render(): string;
  sourcemap?: RawSourceMap;
}

interface StylusError {
  message: string;
  lineno?: number;
  column?: number;
}
