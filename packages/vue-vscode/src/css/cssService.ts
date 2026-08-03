/**
 * CSS language service wrapper for `<style>` blocks in Vue SFCs.
 *
 * Integrates `vscode-css-languageservice` with Verter's virtual file system to provide
 * full CSS intellisense (completions, hover, diagnostics, colors) in `<style>` blocks.
 *
 * - CSS/SCSS/LESS: fed directly to the appropriate language service
 * - Sass/Stylus: transpiled via project compilers, then fed to CSS service
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
import { transpile, type TranspileResult } from "./transpiler";
import { resolvePreprocessor, type PreprocessorCache } from "./preprocessorResolver";

// ── Language service singletons ──────────────────────────────────

const cssService = getCSSLanguageService();
const scssService = getSCSSLanguageService();
const lessService = getLESSLanguageService();

function getServiceForLang(lang: StyleLang): CSSLanguageService | null {
  switch (lang) {
    case "css":
    case "postcss":
      return cssService;
    case "scss":
      return scssService;
    case "less":
      return lessService;
    default:
      // sass, stylus — handled via transpilation to CSS
      return null;
  }
}

// ── Virtual file cache ───────────────────────────────────────────

interface DocumentCache {
  version: number;
  openEpoch: string;
  availability: DocumentStructureResponseV1["kind"] | "transportUnavailable";
  blocks: StyleBlockInfo[];
  source: string;
  /** Captured from the admitted `available` structure (R2-B-04): style
   * overrides computed against this structure are revision-bound to it. */
  documentRevisionToken?: string;
  artifactToken?: string;
  /** Keyed by style block index — transpiled CSS for preprocessors */
  transpiled: Map<number, TranspileResult>;
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
      const service = this.getServiceForBlock(block, entry);
      const cssDoc = this.getCssDocument(block, entry, uri);
      if (!service || !cssDoc) continue;

      const stylesheet = service.parseStylesheet(cssDoc);
      const diags = service.doValidation(cssDoc, stylesheet);
      if (diags.length > 0) {
        // Map diagnostic ranges back to SFC coordinates
        for (const d of diags) {
          d.range = this.toSfcRange(block, d.range);
        }
        results.push({ blockToken: block.blockToken, diagnostics: diags });
      }
    }

    return results;
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
      const service = this.getServiceForBlock(block, entry);
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

    const service = this.getServiceForBlock(block, entry);
    const cssDoc = this.getCssDocument(block, entry, uri);
    if (!service || !cssDoc) return null;

    return { block, service, cssDoc };
  }

  private getServiceForBlock(
    block: StyleBlockInfo,
    entry: DocumentCache,
  ): CSSLanguageService | null {
    // For preprocessors that need transpilation, check if we have transpiled output
    if (block.lang === "sass" || block.lang === "stylus") {
      return entry.transpiled.has(block.legacyPreprocessorIndex) ? cssService : null;
    }
    return getServiceForLang(block.lang);
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

    // For transpiled languages, use transpiled CSS
    if (block.lang === "sass" || block.lang === "stylus") {
      const transpiled = entry.transpiled.get(block.legacyPreprocessorIndex);
      if (!transpiled) return null;
      return TextDocument.create(
        `${sfcUri}.style.${block.blockToken}.css`,
        "css",
        1,
        transpiled.css,
      );
    }

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
    const captured =
      admittedAvailable && response !== null && response.kind === "available"
        ? {
            documentRevisionToken: response.structure.documentRevisionToken,
            artifactToken: response.structure.artifactToken,
          }
        : undefined;
    const transpiled = new Map<number, TranspileResult>();

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
        transpiled.set(block.legacyPreprocessorIndex, result);

        // Send transpiled CSS back to the host for analysis — bound to the
        // captured structure tokens, and only while the document is still
        // the exact revision the transpile ran against (R2-B-04).
        await this.applyStyleOverride(
          uri,
          block.legacyPreprocessorIndex,
          result,
          version,
          openEpoch,
          captured,
        );
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

    // Update diagnostics atomically for this URI (clears stale entries) —
    // only from a current, admitted-available invocation. A non-available
    // structure knows NOTHING about the blocks, so it has nothing
    // authoritative to publish or clear.
    if (admittedAvailable && stillCurrent()) {
      try {
        const vscodeUri = vscode.Uri.parse(uri);
        this.diagnostics.set(vscodeUri, missingDiags);
      } catch {
        // URI parsing may fail for non-file URIs; ignore
      }
    }

    const entry: DocumentCache = {
      version,
      openEpoch,
      availability: admitted && response ? response.kind : "transportUnavailable",
      blocks,
      source,
      ...(captured ?? {}),
      transpiled,
    };
    // Cache only current, admitted-available results: a transient
    // non-available must be re-queried on the next demand — never sticky
    // for the whole (version, openEpoch) — and a stale invocation must not
    // overwrite the newer revision's entry.
    if (admittedAvailable && stillCurrent()) {
      this.cache.set(uri, entry);
    }
    return entry;
  }

  /**
   * Send a preprocessor-compiled style override to the Rust LSP host.
   * This updates the host's analysis with the transpiled CSS.
   *
   * Revision-bound (R2-B-04): the transpile is async, so the document may
   * have moved while it ran. The result is dropped client-side when the live
   * document no longer matches the captured version/epoch, and the request
   * carries the captured structure tokens so the server independently
   * refuses a mismatched-revision apply.
   */
  private async applyStyleOverride(
    uri: string,
    index: number,
    result: TranspileResult,
    version: number,
    openEpoch: string,
    captured?: { documentRevisionToken: string; artifactToken: string },
  ): Promise<void> {
    if (!captured) {
      // The structure tokens are REQUIRED server-side: an apply that cannot
      // carry them would be refused typed — send nothing.
      return;
    }
    const live = vscode.workspace.textDocuments.find((document) => document.uri.toString() === uri);
    if (live?.version !== version || this.getOpenEpoch(uri) !== openEpoch) {
      // Revision A's slow transpile result must not overwrite revision B's
      // state: the newer revision owes its own transpile.
      return;
    }
    try {
      const client = this.getClient();
      await client.sendRequest(RequestType.ApplyStyleOverrides, {
        uri,
        overrides: [
          {
            index,
            code: result.css,
            sourceMap: result.sourceMap ? JSON.stringify(result.sourceMap) : undefined,
          },
        ],
        ...captured,
      });
    } catch {
      // Silently fail — LSP might not support this yet
    }
  }
}
