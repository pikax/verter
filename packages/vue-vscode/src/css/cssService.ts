/**
 * CSS language service wrapper for `<style>` blocks in Vue SFCs.
 *
 * Integrates `vscode-css-languageservice` with Verter's virtual file system to provide
 * full CSS intellisense (completions, hover, diagnostics, colors) in `<style>` blocks.
 *
 * - CSS/SCSS/LESS: the verbatim authored slice is fed directly to the appropriate
 *   language service — the owner-specific editor adapter over authored bytes
 * - Sass/Stylus: transpiled via project compilers; the qualified result's
 *   diagnostics are the block's facts. The generated CSS is NEVER parsed here:
 *   preprocessed bytes belong to their qualified stage, and a second CSS parse
 *   of them would report coordinates that address nothing the user wrote.
 */

import {
  getCSSLanguageService,
  getSCSSLanguageService,
  getLESSLanguageService,
  type LanguageService as CSSLanguageService,
  type CompletionList,
  type Hover,
  type Diagnostic as CSSDiagnostic,
  type ColorInformation,
  type ColorPresentation,
  type Color,
  type DocumentHighlight,
  type TextDocument as CSSTextDocument,
} from "vscode-css-languageservice";
import { TextDocument } from "vscode-languageserver-textdocument";
import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";
import type { PatchClient, DocumentStructureResponseV1 } from "@verter/language-shared";
import { RequestType } from "@verter/language-shared";
import {
  styleBlocksFromStructure,
  findStyleBlockAt,
  type StyleBlockInfo,
  type StyleLang,
} from "./styleStructure";
import { directStyleDocumentText } from "./styleDocumentText";
import { transpile, type QualifiedStyleResult } from "./transpiler";
import { resolvePreprocessor, type PreprocessorCache } from "./preprocessorResolver";

// ── Language service singletons ──────────────────────────────────

const cssService = getCSSLanguageService();
const scssService = getSCSSLanguageService();
const lessService = getLESSLanguageService();

function getServiceForLang(lang: StyleLang | null): CSSLanguageService | null {
  switch (lang) {
    case "css":
    case "postcss":
      return cssService;
    case "scss":
      return scssService;
    case "less":
      return lessService;
    default:
      // sass and stylus go through transpilation to CSS; `null` is a dialect
      // this client cannot address and is served nothing.
      return null;
  }
}

// ── Virtual file cache ───────────────────────────────────────────

interface DocumentCache {
  version: number;
  openEpoch: string;
  availability: DocumentStructureResponseV1["kind"] | "transportUnavailable" | "staleInvocation";
  blocks: StyleBlockInfo[];
  source: string;
  /** Keyed by sealed block token — qualified preprocessed results. */
  transpiled: Map<string, QualifiedStyleResult>;
}

// ── CssService class ─────────────────────────────────────────────

export class CssService {
  private cache = new Map<string, DocumentCache>();
  private preprocessors: PreprocessorCache = {};
  /** Track which preprocessor warnings have been shown (show at most once per session). */
  private warnedMissing = new Set<string>();
  /** Inline diagnostics for missing preprocessor packages (sass, stylus). */
  private diagnostics = vscode.languages.createDiagnosticCollection("verter-preprocessor");
  private requestNonce = 0;

  constructor(
    private getClient: () => PatchClient<LanguageClient>,
    private workspacePath: string | undefined,
    private getOpenEpoch: (uri: string) => string,
  ) {
    // Pre-resolve preprocessors from the workspace's node_modules
    if (workspacePath) {
      resolvePreprocessor("sass", workspacePath, this.preprocessors);
      resolvePreprocessor("stylus", workspacePath, this.preprocessors);
    }
  }

  // ── Public API ─────────────────────────────────────────────────

  /**
   * Get CSS completions at the given position in a Vue SFC.
   * Returns null if the position is not inside a `<style>` block.
   */
  async doComplete(
    uri: string,
    source: string,
    version: number,
    line: number,
    character: number,
  ): Promise<CompletionList | null> {
    const { block, service, cssDoc } =
      (await this.prepareAt(uri, source, version, line, character)) ?? {};
    if (!block || !service || !cssDoc) return null;

    const cssPos = this.toCssPosition(block, line, character);
    const stylesheet = service.parseStylesheet(cssDoc);
    const result = service.doComplete(cssDoc, cssPos, stylesheet);

    // Map textEdit ranges back to SFC coordinates
    for (const item of result.items) {
      if (item.textEdit) {
        if ("range" in item.textEdit) {
          item.textEdit.range = this.toSfcRange(block, item.textEdit.range);
        } else {
          // InsertReplaceEdit
          item.textEdit.insert = this.toSfcRange(block, item.textEdit.insert);
          item.textEdit.replace = this.toSfcRange(block, item.textEdit.replace);
        }
      }
      if (item.additionalTextEdits) {
        for (const edit of item.additionalTextEdits) {
          edit.range = this.toSfcRange(block, edit.range);
        }
      }
    }

    return result;
  }

  /**
   * Get hover information at the given position.
   */
  async doHover(
    uri: string,
    source: string,
    version: number,
    line: number,
    character: number,
  ): Promise<Hover | null> {
    const { block, service, cssDoc } =
      (await this.prepareAt(uri, source, version, line, character)) ?? {};
    if (!block || !service || !cssDoc) return null;

    const cssPos = this.toCssPosition(block, line, character);
    const stylesheet = service.parseStylesheet(cssDoc);
    const hover = service.doHover(cssDoc, cssPos, stylesheet);
    if (!hover) return null;

    // Map hover range back to SFC coordinates
    if (hover.range) {
      hover.range = this.toSfcRange(block, hover.range);
    }
    return hover;
  }

  /**
   * Get CSS diagnostics for all style blocks in the document.
   *
   * FAIL CLOSED on availability (TE-C-11): returns `null` — NOT an empty
   * array — unless the structure response for THIS version was an admitted
   * `available`. An empty array is a successful "genuinely clean" validation
   * the publisher may publish (clearing prior diagnostics); `null` means the
   * structure was stale/unavailable/closed or the transport failed, and the
   * publisher must keep the last-known real diagnostics and publish nothing.
   */
  async doValidation(
    uri: string,
    source: string,
    version: number,
  ): Promise<Array<{ blockToken: string; diagnostics: CSSDiagnostic[] }> | null> {
    const entry = await this.ensureCache(uri, source, version);
    if (entry.availability !== "available") {
      return null;
    }
    const results: Array<{ blockToken: string; diagnostics: CSSDiagnostic[] }> = [];

    for (const block of entry.blocks) {
      // A preprocessor dialect's only CSS-side facts are its qualified
      // result's diagnostics — reported against the bytes the tool CONSUMED
      // (the authored slice), which is what makes the block's own line
      // arithmetic the right map. A compile failure recovers here instead of
      // surfacing as silence; a clean compile is a clean block.
      const preprocessorDiags = this.preprocessorDiagnostics(block, entry);

      // Only authored dialect slices reach a language service: the adapter's
      // single parse is over the verbatim bytes the user wrote. Preprocessed
      // output is never parsed (getServiceForLang has no service for a
      // preprocessor dialect), so no generated-coordinate ranges exist to
      // anchor or mis-map.
      const service = getServiceForLang(block.lang);
      const cssDoc = this.getCssDocument(block, entry, uri);
      const diags =
        service && cssDoc ? service.doValidation(cssDoc, service.parseStylesheet(cssDoc)) : [];
      for (const d of diags) d.range = this.toSfcRange(block, d.range);

      const blockDiags = [...preprocessorDiags, ...diags];
      if (blockDiags.length > 0) {
        results.push({ blockToken: block.blockToken, diagnostics: blockDiags });
      }
    }

    return results;
  }

  /**
   * The block's preprocessor-reported diagnostics, mapped into SFC
   * coordinates.
   *
   * The mapping is chosen from the stage the diagnostic names, not assumed.
   * `"authored"` positions are in the verbatim carrier slice this client fed
   * the tool, so the block's own line arithmetic is exact. There is no
   * preprocessed → SFC map here (the generated CSS is not what the user is
   * looking at, and a failed compile produced none at all), so anything else
   * anchors at the block's first position rather than being run through
   * arithmetic that does not address it.
   */
  private preprocessorDiagnostics(block: StyleBlockInfo, entry: DocumentCache): CSSDiagnostic[] {
    const transpiled = entry.transpiled.get(block.blockToken);
    if (!transpiled) return [];
    const blockStart = this.toSfcPosition(block, { line: 0, character: 0 });
    return transpiled.diagnostics.map((diagnostic) => {
      const sfc =
        diagnostic.stage === "authored" && diagnostic.position
          ? this.toSfcPosition(block, diagnostic.position)
          : blockStart;
      return {
        range: { start: sfc, end: sfc },
        message: diagnostic.message,
        severity: diagnostic.severity === "error" ? 1 : diagnostic.severity === "warning" ? 2 : 3,
        source: transpiled.producer?.identity ?? "preprocessor",
      } satisfies CSSDiagnostic;
    });
  }

  /**
   * Find document colors in all style blocks.
   */
  async findDocumentColors(
    uri: string,
    source: string,
    version: number,
  ): Promise<ColorInformation[]> {
    const entry = await this.ensureCache(uri, source, version);
    const allColors: ColorInformation[] = [];

    for (const block of entry.blocks) {
      const service = getServiceForLang(block.lang);
      const cssDoc = this.getCssDocument(block, entry, uri);
      if (!service || !cssDoc) continue;

      const stylesheet = service.parseStylesheet(cssDoc);
      const colors = service.findDocumentColors(cssDoc, stylesheet);

      // Map positions back to SFC
      for (const color of colors) {
        color.range = this.toSfcRange(block, color.range);
        allColors.push(color);
      }
    }

    return allColors;
  }

  /**
   * Get color presentations for a color at a position.
   */
  async getColorPresentations(
    uri: string,
    source: string,
    version: number,
    color: Color,
    line: number,
    character: number,
  ): Promise<ColorPresentation[]> {
    const { block, service, cssDoc } =
      (await this.prepareAt(uri, source, version, line, character)) ?? {};
    if (!block || !service || !cssDoc) return [];

    const stylesheet = service.parseStylesheet(cssDoc);
    // Create a dummy range for the color
    const cssPos = this.toCssPosition(block, line, character);
    const range = { start: cssPos, end: cssPos };
    return service.getColorPresentations(cssDoc, stylesheet, color, range);
  }

  /**
   * Find document highlights at the given position.
   */
  async findDocumentHighlights(
    uri: string,
    source: string,
    version: number,
    line: number,
    character: number,
  ): Promise<DocumentHighlight[]> {
    const { block, service, cssDoc } =
      (await this.prepareAt(uri, source, version, line, character)) ?? {};
    if (!block || !service || !cssDoc) return [];

    const cssPos = this.toCssPosition(block, line, character);
    const stylesheet = service.parseStylesheet(cssDoc);
    const highlights = service.findDocumentHighlights(cssDoc, cssPos, stylesheet);

    // Map positions back to SFC
    return highlights.map((h) => ({
      ...h,
      range: this.toSfcRange(block, h.range),
    }));
  }

  dispose(): void {
    this.cache.clear();
    this.diagnostics.dispose();
  }

  // ── Internal helpers ───────────────────────────────────────────

  private async prepareAt(
    uri: string,
    source: string,
    version: number,
    line: number,
    character: number,
  ): Promise<{
    block: StyleBlockInfo;
    service: CSSLanguageService;
    cssDoc: CSSTextDocument;
  } | null> {
    const entry = await this.ensureCache(uri, source, version);
    const block = findStyleBlockAt(entry.blocks, source, line, character);
    if (!block) return null;

    // Cursor positions and returned ranges are authored coordinates, and only
    // an authored dialect has a service here: a preprocessor dialect has no
    // CSS slice to serve positional features on — its facts are the qualified
    // diagnostics doValidation already maps.
    const service = getServiceForLang(block.lang);
    const cssDoc = this.getCssDocument(block, entry, uri);
    if (!service || !cssDoc) return null;

    return { block, service, cssDoc };
  }

  private getCssDocument(
    block: StyleBlockInfo,
    entry: DocumentCache,
    sfcUri: string,
  ): CSSTextDocument | null {
    // External-src blocks yield NO inline slice (R2-B-03): the inline bytes
    // are framework-ignored — never validate, hover, or color them as if
    // they were available content. Typed unavailable, fail closed.
    if (block.externalSrc) return null;

    // A dialect this client cannot address gets no virtual document at all:
    // there is no language id to open it under, and opening it as CSS anyway
    // is what produced fabricated CSS errors for non-CSS syntax. A preprocessor
    // dialect is that case too — its authored bytes are not CSS, and its
    // preprocessed bytes are qualified output this adapter never parses.
    if (block.lang === null) return null;
    if (block.lang === "sass" || block.lang === "stylus") return null;

    // For direct languages (css, scss, less, postcss), the CSS service MUST
    // parse the VERBATIM authored slice: `toCssPosition`/`toSfcPosition` are
    // pure line arithmetic, valid only for byte-identical text. The compiled
    // style virtual file (scoped selector rewrites, v-bind() → var() rewrites,
    // trimming) shifts lines/columns and mis-maps every range — the color
    // decorator rendered on the FIRST class name instead of the color value.
    const code = directStyleDocumentText(entry.source, block);

    const langId = block.lang === "postcss" ? "css" : block.lang;
    return TextDocument.create(`${sfcUri}.style.${block.blockToken}.${langId}`, langId, 1, code);
  }

  /**
   * Convert an SFC position to a position within the virtual CSS document.
   */
  private toCssPosition(
    block: StyleBlockInfo,
    sfcLine: number,
    sfcChar: number,
  ): { line: number; character: number } {
    const cssLine = sfcLine - block.contentStartLine;
    const cssChar = cssLine === 0 ? sfcChar - block.contentStartColumn : sfcChar;
    return { line: cssLine, character: cssChar };
  }

  /**
   * Convert a CSS document range back to an SFC range.
   */
  private toSfcRange(
    block: StyleBlockInfo,
    range: { start: { line: number; character: number }; end: { line: number; character: number } },
  ): { start: { line: number; character: number }; end: { line: number; character: number } } {
    return {
      start: this.toSfcPosition(block, range.start),
      end: this.toSfcPosition(block, range.end),
    };
  }

  private toSfcPosition(
    block: StyleBlockInfo,
    pos: { line: number; character: number },
  ): { line: number; character: number } {
    const sfcLine = pos.line + block.contentStartLine;
    const sfcChar = pos.line === 0 ? pos.character + block.contentStartColumn : pos.character;
    return { line: sfcLine, character: sfcChar };
  }

  /**
   * Ensure cache is up to date for the given document.
   * Fetches virtual files from the LSP and transpiles preprocessors as needed.
   */
  private async ensureCache(uri: string, source: string, version: number): Promise<DocumentCache> {
    const existing = this.cache.get(uri);
    const openEpoch = this.getOpenEpoch(uri);
    if (existing && existing.version === version && existing.openEpoch === openEpoch) {
      return existing;
    }

    const requestToken = `${openEpoch}:${version}:${++this.requestNonce}`;
    let response: DocumentStructureResponseV1 | null = null;
    try {
      response = await this.getClient().sendRequest(RequestType.GetDocumentStructure, {
        requestToken,
        textDocument: { uri },
        clientOpenEpoch: openEpoch,
        expectedClientVersion: version,
      });
    } catch {
      response = null;
    }
    const live = vscode.workspace.textDocuments.find((document) => document.uri.toString() === uri);
    const admitted =
      response !== null &&
      response.requestToken === requestToken &&
      response.clientOpenEpoch === openEpoch &&
      response.expectedClientVersion === version &&
      live?.version === version &&
      this.getOpenEpoch(uri) === openEpoch;
    const blocks = admitted && response ? styleBlocksFromStructure(source, response) : [];
    const admittedAvailable = admitted && response !== null && response.kind === "available";
    const transpiled = new Map<string, QualifiedStyleResult>();

    // Transpile preprocessors if needed (resolved from workspace node_modules)
    // Collect missing-preprocessor diagnostics for this URI atomically.
    const missingDiags: vscode.Diagnostic[] = [];

    // Every post-transpile-await side effect (warning, diagnostics publish,
    // cache write) runs only for a still-current invocation: the document
    // may have moved (or been reopened) while an await was in flight, and a
    // STALE invocation must never warn, clear, or replace a newer
    // revision's state.
    const stillCurrent = () => {
      const liveNow = vscode.workspace.textDocuments.find(
        (document) => document.uri.toString() === uri,
      );
      return liveNow?.version === version && this.getOpenEpoch(uri) === openEpoch;
    };

    for (const block of blocks) {
      // External-src blocks have no inline content to transpile — the
      // inline slice is framework-ignored and the host REJECTS overrides
      // targeting a deferred block (R2-B-03). Send nothing.
      if (block.externalSrc) continue;
      if (block.lang !== "sass" && block.lang !== "stylus") continue;

      // Re-resolve if workspace path changed or wasn't available at construction
      if (this.workspacePath) {
        resolvePreprocessor(block.lang, this.workspacePath, this.preprocessors);
      }

      const authored = directStyleDocumentText(source, block);
      const result = await transpile(authored, block.lang, uri, this.preprocessors);
      if (result) {
        transpiled.set(block.blockToken, result);
      } else {
        // Emit an inline diagnostic on the lang="..." attribute
        if (block.langAttributeRange) {
          const range = new vscode.Range(
            new vscode.Position(
              block.langAttributeRange.startLine,
              block.langAttributeRange.startCol,
            ),
            new vscode.Position(block.langAttributeRange.endLine, block.langAttributeRange.endCol),
          );
          const diag = new vscode.Diagnostic(
            range,
            `"${block.lang}" is not installed. CSS intellisense for <style lang="${block.lang}"> ` +
              `requires "${block.lang}" as a project dependency.`,
            vscode.DiagnosticSeverity.Error,
          );
          diag.source = "verter";
          missingDiags.push(diag);
        }

        // Show a one-time warning message as secondary guidance — only from
        // a still-current invocation: a stale post-transpile invocation must
        // neither warn nor mark the lang as warned, which would permanently
        // suppress a later relevant warning.
        if (stillCurrent() && !this.warnedMissing.has(block.lang)) {
          this.warnedMissing.add(block.lang);
          vscode.window.showWarningMessage(
            `Verter: "${block.lang}" is not installed in the workspace. ` +
              `CSS intellisense for <style lang="${block.lang}"> blocks requires ` +
              `"${block.lang}" to be installed as a project dependency.`,
          );
        }
      }
    }

    // One post-await admission decision governs every remaining side effect
    // AND the returned value: the transpile/override awaits above may have
    // outlived the revision this invocation ran against.
    const current = stillCurrent();

    // Update diagnostics atomically for this URI (clears stale entries) —
    // only from a current, admitted-available invocation. A non-available
    // structure knows NOTHING about the blocks, so it has nothing
    // authoritative to publish or clear.
    if (admittedAvailable && current) {
      try {
        const vscodeUri = vscode.Uri.parse(uri);
        this.diagnostics.set(vscodeUri, missingDiags);
      } catch {
        // URI parsing may fail for non-file URIs; ignore
      }
    }

    // A STALE invocation returns a typed miss (B-29): its admitted structure
    // belongs to a revision the document has left behind. Handing it back as
    // "available" would let a caller validate old blocks against new text and
    // publish the result (an empty validation would even CLEAR the newer
    // revision's real diagnostics). Callers fail closed on non-available.
    if (!current) {
      return {
        version,
        openEpoch,
        availability: "staleInvocation",
        blocks: [],
        source,
        transpiled: new Map(),
      };
    }

    const entry: DocumentCache = {
      version,
      openEpoch,
      availability: admitted && response ? response.kind : "transportUnavailable",
      blocks,
      source,
      transpiled,
    };
    // Cache only current, admitted-available results: a transient
    // non-available must be re-queried on the next demand — never sticky
    // for the whole (version, openEpoch) — and a stale invocation must not
    // overwrite the newer revision's entry.
    if (admittedAvailable) {
      this.cache.set(uri, entry);
    }
    return entry;
  }
}
