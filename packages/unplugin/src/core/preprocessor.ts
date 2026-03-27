import type { HostPreprocessorRequest } from "@verter/native";
import type { BlockPreprocessor } from "./types";

interface PreprocessResult {
  code: string;
  sourceMap?: string;
}

/**
 * Preprocess a single block that requires external compilation.
 *
 * - **template**: dynamic import of lang compiler (e.g., `pug`)
 * - **script**: dynamic import of lang compiler (e.g., `coffeescript`)
 * - **style**: delegates to Vite's `preprocessCSS()` when available
 * - **custom**: checks user-provided `customBlocks` map, falls back to auto-detect by lang
 *
 * @returns Preprocessed `{ code, sourceMap? }`, or `null` if no preprocessor is available.
 */
export async function preprocessBlock(
  req: HostPreprocessorRequest,
  filename: string,
  viteConfig: unknown | null,
  customBlockHandlers?: Record<string, BlockPreprocessor>,
): Promise<PreprocessResult | null> {
  switch (req.blockType) {
    case "template":
      return preprocessTemplate(req.lang, req.content, filename);
    case "script":
      return preprocessScript(req.lang, req.content, filename);
    case "style":
      return preprocessStyle(req.lang, req.content, filename, viteConfig);
    case "custom":
      return preprocessCustom(req.lang, req.content, filename, customBlockHandlers);
    default:
      return null;
  }
}

export async function preprocessTemplate(
  lang: string,
  content: string,
  _filename: string,
): Promise<PreprocessResult | null> {
  const lower = lang.toLowerCase();
  if (lower === "pug" || lower === "jade") {
    try {
      // @ts-expect-error — pug is an optional peer dependency
      const pug = await import("pug");
      const html = pug.render(content, { filename: _filename });
      return { code: html };
    } catch (e: unknown) {
      console.warn(
        `[verter] Failed to preprocess template lang="${lang}": ${e instanceof Error ? e.message : e}`,
      );
      return null;
    }
  }
  console.warn(
    `[verter] No preprocessor available for template lang="${lang}". Install the "${lang}" package.`,
  );
  return null;
}

export async function preprocessScript(
  lang: string,
  content: string,
  _filename: string,
): Promise<PreprocessResult | null> {
  const lower = lang.toLowerCase();
  if (lower === "coffee" || lower === "coffeescript") {
    try {
      // @ts-expect-error — coffeescript is an optional peer dependency
      const coffee = await import("coffeescript");
      const result = coffee.compile(content, {
        bare: true,
        sourceMap: true,
        filename: _filename,
      });
      if (typeof result === "string") {
        return { code: result };
      }
      return {
        code: result.js,
        sourceMap: result.v3SourceMap,
      };
    } catch (e: unknown) {
      console.warn(
        `[verter] Failed to preprocess script lang="${lang}": ${e instanceof Error ? e.message : e}`,
      );
      return null;
    }
  }
  console.warn(
    `[verter] No preprocessor available for script lang="${lang}". Install the "${lang}" package.`,
  );
  return null;
}

export async function preprocessStyle(
  lang: string,
  content: string,
  filename: string,
  viteConfig: unknown | null,
): Promise<PreprocessResult | null> {
  if (viteConfig) {
    try {
      const { preprocessCSS } = await import("vite");
      const result = await preprocessCSS(
        content,
        `${filename}.${lang}`,
        viteConfig as Parameters<typeof preprocessCSS>[2],
      );
      return {
        code: result.code,
        sourceMap: result.map ? JSON.stringify(result.map) : undefined,
      };
    } catch (e: unknown) {
      console.warn(
        `[verter] Failed to preprocess style lang="${lang}": ${e instanceof Error ? e.message : e}`,
      );
      return null;
    }
  }
  console.warn(
    `[verter] Style preprocessing for lang="${lang}" requires Vite. ` +
      `Other bundlers are not yet supported for style preprocessing.`,
  );
  return null;
}

export async function preprocessCustom(
  lang: string,
  content: string,
  filename: string,
  customBlockHandlers?: Record<string, BlockPreprocessor>,
): Promise<PreprocessResult | null> {
  // Check user-provided handler first (keyed by block type — but for custom
  // blocks we only have lang here, so we match by lang)
  if (customBlockHandlers) {
    for (const [_type, handler] of Object.entries(customBlockHandlers)) {
      // The handler is keyed by custom block type (e.g., "i18n"), but at this
      // point we don't have the block type — only the lang. For now, try all
      // handlers and use the first non-null result.
      const result = await handler(content, lang, filename);
      if (result) return result;
    }
  }
  return null;
}
