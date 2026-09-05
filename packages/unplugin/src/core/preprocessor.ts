import type { HostBlockContentCapturedEchoFields, HostPreprocessorRequest } from "@verter/native";
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
 * - **style**: delegates to Vite's `preprocessCSS()` when available;
 *   otherwise compiles SCSS/Sass through the optional `sass` package
 * - **custom**: checks user-provided `customBlocks` map, falls back to auto-detect by lang
 *
 * @returns Preprocessed `{ code, sourceMap? }`, or `null` if no preprocessor is available.
 * @throws When an available preprocessor rejects the block (e.g. an SCSS
 *   syntax error) — that is a broken input, not a missing tool, so the
 *   compiler's message surfaces instead of the host's opaque
 *   `ProcessedContentRequired` refusal.
 */
export async function preprocessBlock(
  req: HostPreprocessorRequest,
  filename: string,
  viteConfig: unknown | null,
  customBlockHandlers?: Record<string, BlockPreprocessor>,
): Promise<PreprocessResult | null> {
  switch (req.contentClass) {
    case "template":
      return preprocessTemplate(req.lang, req.content, filename);
    case "script":
      return preprocessScript(req.lang, req.content, filename);
    case "style":
      return preprocessStyle(req.lang, req.content, filename, viteConfig);
    case "custom":
      return preprocessCustom(req, filename, customBlockHandlers);
    default:
      return null;
  }
}

/** Copy the exact host-captured echo without allowing payload fields into it. */
export function copyCapturedBlockContentEcho(
  request: HostPreprocessorRequest,
): HostBlockContentCapturedEchoFields {
  return {
    correlationToken: request.correlationToken,
    blockToken: request.blockToken,
    ownerRevision: request.ownerRevision,
    artifactToken: request.artifactToken,
    expectedLanguage: request.expectedLanguage,
    priorBasisToken: request.priorBasisToken,
    basisToken: request.basisToken,
  };
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
  // Outside Vite no bundler CSS pipeline runs, so the plugin owns SCSS/Sass
  // preprocessing itself: the compiler's style cascade scopes plain CSS
  // only, and a supplied (preprocessed) artifact is the only way authored
  // non-CSS bytes can reach it.
  const lower = lang.toLowerCase();
  if (lower === "scss" || lower === "sass") {
    let sass: typeof import("sass");
    try {
      sass = await import("sass");
    } catch (e: unknown) {
      if (e !== null && typeof e === "object" && "code" in e && e.code === "ERR_MODULE_NOT_FOUND") {
        console.warn(
          `[verter] Style preprocessing for lang="${lang}" requires the "sass" package. ` +
            `Install it to compile ${lower} blocks outside Vite.`,
        );
        return null;
      }
      throw e;
    }

    try {
      const result = sass.compileString(content, {
        syntax: lower === "sass" ? "indented" : "scss",
        sourceMap: true,
        sourceMapIncludeSources: true,
      });
      return {
        code: result.css,
        sourceMap: result.sourceMap ? JSON.stringify(result.sourceMap) : undefined,
      };
    } catch (e: unknown) {
      const message = e instanceof Error ? e.message : String(e);
      // A genuine compile failure is not "no preprocessor available": if it
      // were swallowed to `null` the host would later refuse the block as
      // `ProcessedContentRequired`, burying the compiler's own message.
      // Surface it instead so a broken SCSS block fails the build saying why.
      throw new Error(
        `[verter] Failed to preprocess style lang="${lang}" in ${filename}: ${message}`,
        { cause: e },
      );
    }
  }
  console.warn(
    `[verter] No preprocessor available for style lang="${lang}" outside Vite. ` +
      `Install the "${lang}" preprocessor or build through Vite.`,
  );
  return null;
}

export async function preprocessCustom(
  request: HostPreprocessorRequest,
  filename: string,
  customBlockHandlers?: Record<string, BlockPreprocessor>,
): Promise<PreprocessResult | null> {
  // Select the user-provided handler by the declared custom-block type.
  if (customBlockHandlers) {
    for (const [_type, handler] of Object.entries(customBlockHandlers)) {
      if (_type === request.customType) {
        const result = await handler(request, filename);
        if (result) return result;
      }
    }
  }
  return null;
}
