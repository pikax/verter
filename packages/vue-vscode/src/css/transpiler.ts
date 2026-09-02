/**
 * Transpile Sass/Stylus to CSS through project-installed compilers.
 *
 * The result is stage-qualified, mirroring the Rust style-identity vocabulary
 * (`verter_css_syntax::stage`): the bytes carry the stage they belong to, the
 * dialect they are now written in, and the tool that produced them. A bare
 * `{ css, sourceMap, errors }` record forced every consumer to re-derive all
 * three, and it made the reported errors easy to drop on the floor — which is
 * exactly what happened: nothing read them, so a Sass compile failure was
 * invisible in the editor.
 */

import type { RawSourceMap } from "source-map";
import type { PreprocessorCache } from "./preprocessorResolver";

/**
 * Where a set of style bytes sits in the authored → preprocessed lineage.
 * This module only ever produces the preprocessed stage; the authored stage is
 * the verbatim carrier slice and needs no producer.
 */
export type StyleStage = "authored" | "preprocessed";

export type StyleSeverity = "error" | "warning" | "info";

/** Which authority observed a diagnostic. */
export type StyleDiagnosticOrigin = "syntax" | "processor";

/**
 * One style diagnostic, qualified by the stage whose coordinate space
 * `position` addresses.
 *
 * The field names the space the position is IN, never the authority that
 * reported it — that is `origin`. A preprocessor reports positions in the bytes
 * it CONSUMED, so its diagnostics are `"authored"` even though its output is
 * `"preprocessed"`. Naming the stage is what lets the consumer decide which
 * mapping to apply; a consumer that assumes one instead is the guessing this
 * shape exists to remove.
 *
 * Mirrors `verter_css_syntax::StyleDiagnostic`, including that rule.
 */
export interface StyleDiagnostic {
  stage: StyleStage;
  origin: StyleDiagnosticOrigin;
  severity: StyleSeverity;
  message: string;
  /** Zero-based line/character in `stage`'s own space, when reported. */
  position?: { line: number; character: number };
}

/** Identity of an external preprocessor. */
export interface ExternalStyleProducer {
  /** Non-empty by construction — see `namedProducer`. */
  identity: string;
  version?: string;
}

/**
 * Style bytes together with the identity that makes them addressable.
 *
 * `producer` is `null` only for an external tool that supplied no identity,
 * which is a real recordable state and distinct from a named one. There is no
 * shape here for bytes with no stage at all.
 */
export interface QualifiedStyleResult {
  stage: StyleStage;
  dialect: "css";
  producer: ExternalStyleProducer | null;
  code: string;
  sourceMap: RawSourceMap | null;
  diagnostics: StyleDiagnostic[];
}

function namedProducer(identity: string, version: unknown): ExternalStyleProducer | null {
  const trimmed = identity.trim();
  if (trimmed.length === 0) return null;
  const versionText = typeof version === "string" ? version.trim() : "";
  return versionText.length > 0
    ? { identity: trimmed, version: versionText }
    : { identity: trimmed };
}

/**
 * dart-sass has no version accessor; `info` is a multi-line banner whose first
 * line is `dart-sass\t<version>\t(Sass Compiler)\t[Dart]`.
 *
 * Storing the whole banner in a field named `version` would make provenance
 * read as exact when it is a paragraph, so the version is taken only when the
 * banner is in the shape that actually carries one, and is otherwise absent.
 */
function dartSassVersion(info: unknown): string | undefined {
  if (typeof info !== "string") return undefined;
  const [name, version] = info.split("\n", 1)[0].split("\t");
  if (name?.trim() !== "dart-sass") return undefined;
  const trimmed = version?.trim() ?? "";
  return /^\d+\.\d+\.\d+/.test(trimmed) ? trimmed : undefined;
}

/**
 * The tool reports against the bytes it CONSUMED, which are the authored
 * carrier slice — never the CSS it produced, which for a failed compile does
 * not exist at all.
 */
function processorError(
  message: string,
  position?: { line: number; character: number },
): StyleDiagnostic {
  return {
    stage: "authored",
    origin: "processor",
    severity: "error",
    message,
    ...(position ? { position } : {}),
  };
}

/**
 * Transpile preprocessor source to CSS.
 *
 * @param content - The preprocessor source code (the authored carrier slice).
 * @param lang - "sass" or "stylus".
 * @param filePath - URI of the parent Vue file (used for `@import` resolution).
 * @param cache - Preprocessor module cache.
 * @returns The qualified preprocessed result, or `null` when the compiler is
 * not installed — which is a missing capability, not a failed compile, and the
 * caller reports it differently.
 */
export async function transpile(
  content: string,
  lang: "sass" | "stylus",
  filePath: string,
  cache: PreprocessorCache,
): Promise<QualifiedStyleResult | null> {
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
): QualifiedStyleResult | null {
  const sass = cache.sass as SassModule | undefined;
  if (!sass) return null;
  const producer = namedProducer("sass", dartSassVersion(sass.info));

  try {
    const result = sass.compileString(content, {
      syntax: "indented" as const,
      sourceMap: true,
      url: filePathToUrl(filePath),
    });

    return {
      stage: "preprocessed",
      dialect: "css",
      producer,
      code: result.css,
      sourceMap: result.sourceMap ?? null,
      diagnostics: [],
    };
  } catch (err: unknown) {
    const error = err as SassError;
    // dart-sass reports zero-based line/column in the source it consumed —
    // the authored slice this function was handed, which is the space
    // `processorError` stamps.
    const start = error.span?.start;
    return {
      stage: "preprocessed",
      dialect: "css",
      producer,
      code: "",
      sourceMap: null,
      diagnostics: [
        processorError(
          error.message ?? String(err),
          typeof start?.line === "number"
            ? { line: start.line, character: start.column ?? 0 }
            : undefined,
        ),
      ],
    };
  }
}

function transpileStylus(
  content: string,
  filePath: string,
  cache: PreprocessorCache,
): QualifiedStyleResult | null {
  const stylusModule = cache.stylus as StylusModule | undefined;
  if (!stylusModule) return null;
  const producer = namedProducer("stylus", stylusModule.version);

  try {
    const renderer = stylusModule(content)
      .set("filename", filePathToFsPath(filePath))
      .set("sourcemap", { comment: false, basePath: "." });

    const css = renderer.render();
    const sourceMap = renderer.sourcemap as RawSourceMap | undefined;

    return {
      stage: "preprocessed",
      dialect: "css",
      producer,
      code: css,
      sourceMap: sourceMap ?? null,
      diagnostics: [],
    };
  } catch (err: unknown) {
    const error = err as StylusError;
    // Stylus reports one-based line/column; the shared vocabulary is
    // zero-based, so it is normalised here, at the producer that knows.
    return {
      stage: "preprocessed",
      dialect: "css",
      producer,
      code: "",
      sourceMap: null,
      diagnostics: [
        processorError(
          error.message ?? String(err),
          typeof error.lineno === "number"
            ? {
                line: Math.max(0, error.lineno - 1),
                character: Math.max(0, (error.column ?? 1) - 1),
              }
            : undefined,
        ),
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
  /** dart-sass exposes its version banner here. */
  info?: string;
}

interface SassError {
  message: string;
  span?: { start?: { line?: number; column?: number } };
}

interface StylusModule {
  (source: string): StylusRenderer;
  version?: string;
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
