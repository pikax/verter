import type { HostBlockContentCapturedEchoFields, HostPreprocessorRequest } from "@verter/native";
import { dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import type { BlockPreprocessor } from "./types";

/** What one external tool produced for one block. */
export interface PreprocessResult {
  code: string;
  sourceMap?: string;
  /** The tool that ran, e.g. `sass`. */
  processor: string;
  /** Its version, when it states one. */
  version: string | null;
  /** The options the tool ran with, as plain data built in one fixed key order. */
  config: unknown;
  /** Files the tool read besides the block itself, as absolute paths. */
  dependencies: string[];
}

/** `true` when `error` is the failure of a missing optional package. */
function isMissingModule(error: unknown): boolean {
  return (
    error !== null &&
    typeof error === "object" &&
    "code" in error &&
    error.code === "ERR_MODULE_NOT_FOUND"
  );
}

/** The error of a tool that is present and rejected the block. */
function preprocessFailure(
  contentClass: string,
  lang: string,
  filename: string,
  cause: unknown,
): Error {
  const message = cause instanceof Error ? cause.message : String(cause);
  const failure = new Error(
    `[verter] Failed to preprocess ${contentClass} lang="${lang}" in ${filename}: ${message}`,
  );
  // Attached after construction rather than through the options bag: this
  // package's tsconfig targets ES2020, whose `Error` constructor declares no
  // second parameter, and the cause chain has to survive for a caller that
  // inspects it.
  (failure as Error & { cause?: unknown }).cause = cause;
  return failure;
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
    let pug: { render(source: string, options: { filename: string }): string };
    try {
      // @ts-expect-error — pug is an optional peer dependency
      pug = await import("pug");
    } catch (e: unknown) {
      if (!isMissingModule(e)) throw preprocessFailure("template", lang, _filename, e);
      console.warn(
        `[verter] No preprocessor available for template lang="${lang}". Install the "${lang}" package.`,
      );
      return null;
    }
    try {
      const html = pug.render(content, { filename: _filename });
      return {
        code: html,
        processor: "pug",
        version: null,
        config: { lang: lower },
        dependencies: [],
      };
    } catch (e: unknown) {
      throw preprocessFailure("template", lang, _filename, e);
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
    let coffee: {
      compile(source: string, options: object): string | { js: string; v3SourceMap?: string };
    };
    try {
      // @ts-expect-error — coffeescript is an optional peer dependency
      coffee = await import("coffeescript");
    } catch (e: unknown) {
      if (!isMissingModule(e)) throw preprocessFailure("script", lang, _filename, e);
      console.warn(
        `[verter] No preprocessor available for script lang="${lang}". Install the "${lang}" package.`,
      );
      return null;
    }
    const options = { bare: true, sourceMap: true };
    try {
      const result = coffee.compile(content, { ...options, filename: _filename });
      const base = { processor: "coffeescript", version: null, config: options, dependencies: [] };
      if (typeof result === "string") return { code: result, ...base };
      return { code: result.js, sourceMap: result.v3SourceMap, ...base };
    } catch (e: unknown) {
      throw preprocessFailure("script", lang, _filename, e);
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
        processor: "vite:preprocessCSS",
        version: null,
        config: { lang },
        dependencies: [...(result.deps ?? [])],
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
      if (isMissingModule(e)) {
        console.warn(
          `[verter] Style preprocessing for lang="${lang}" requires the "sass" package. ` +
            `Install it to compile ${lower} blocks outside Vite.`,
        );
        return null;
      }
      throw e;
    }

    const syntax = lower === "sass" ? "indented" : "scss";
    const ownPath = fileURLToPath(pathToFileURL(filename));
    try {
      const result = sass.compileString(content, {
        syntax,
        // Relative `@use`/`@import` resolve against the block's own directory.
        loadPaths: [dirname(filename)],
        // The map names the block by the carrier it came from.
        url: pathToFileURL(filename),
        sourceMap: true,
        sourceMapIncludeSources: true,
      });
      return {
        code: result.css,
        sourceMap: result.sourceMap ? JSON.stringify(result.sourceMap) : undefined,
        processor: "sass",
        version: sassVersion(sass),
        config: { syntax },
        dependencies: result.loadedUrls
          .filter((url) => url.protocol === "file:")
          .map((url) => fileURLToPath(url))
          .filter((path) => path !== ownPath),
      };
    } catch (e: unknown) {
      // A genuine compile failure is not "no preprocessor available": if it
      // were swallowed to `null` the host would later refuse the block as
      // `ProcessedContentRequired`, burying the compiler's own message.
      throw preprocessFailure("style", lang, filename, e);
    }
  }
  console.warn(
    `[verter] No preprocessor available for style lang="${lang}" outside Vite. ` +
      `Install the "${lang}" preprocessor or build through Vite.`,
  );
  return null;
}

/** The version of the imported `sass` module, from its `info` line (`dart-sass<TAB>1.2.3<TAB>...`). */
function sassVersion(sass: typeof import("sass")): string {
  return sass.info.split("\t")[1] ?? "unknown";
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
        if (result) {
          return {
            code: result.code,
            sourceMap: result.sourceMap,
            processor: `custom:${_type}`,
            version: null,
            config: { type: _type },
            dependencies: [],
          };
        }
      }
    }
  }
  return null;
}
