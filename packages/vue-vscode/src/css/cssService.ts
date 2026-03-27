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
import type { PatchClient } from "@verter/language-shared";
import { RequestType } from "@verter/language-shared";
import {
  scanStyleBlocks,
  findStyleBlockAt,
  type StyleBlockInfo,
  type StyleLang,
} from "./styleBlockScanner";
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

interface CachedStyleEntry {
  code: string;
  lang: string;
}

interface DocumentCache {
  version: number;
  blocks: StyleBlockInfo[];
  source: string;
  /** Keyed by style block index */
  virtualFiles: Map<number, CachedStyleEntry>;
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

  constructor(
    private getClient: () => PatchClient<LanguageClient>,
    private workspacePath: string | undefined,
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
   */
  async doValidation(
    uri: string,
    source: string,
    version: number,
  ): Promise<Array<{ blockIndex: number; diagnostics: CSSDiagnostic[] }>> {
    const entry = await this.ensureCache(uri, source, version);
    const results: Array<{ blockIndex: number; diagnostics: CSSDiagnostic[] }> = [];

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
        results.push({ blockIndex: block.index, diagnostics: diags });
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

  /**
   * Check if a position is inside a style block.
   */
  isInStyleBlock(source: string, line: number, character: number): boolean {
    const blocks = scanStyleBlocks(source);
    return findStyleBlockAt(blocks, source, line, character) !== undefined;
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
      return entry.transpiled.has(block.index) ? cssService : null;
    }
    return getServiceForLang(block.lang);
  }

  private getCssDocument(
    block: StyleBlockInfo,
    entry: DocumentCache,
    sfcUri: string,
  ): CSSTextDocument | null {
    // For transpiled languages, use transpiled CSS
    if (block.lang === "sass" || block.lang === "stylus") {
      const transpiled = entry.transpiled.get(block.index);
      if (!transpiled) return null;
      return TextDocument.create(`${sfcUri}.style.${block.index}.css`, "css", 1, transpiled.css);
    }

    // For direct languages (css, scss, less, postcss), use virtual file content.
    // Fall back to extracting content directly from the SFC source when the LSP
    // doesn't provide style virtual files (e.g., when the compile profile doesn't
    // include the STYLE target — the IDE profile only includes TSX).
    const vf = entry.virtualFiles.get(block.index);
    // Use virtual file content only if it's non-empty and has the correct language.
    // Empty code with lang "js" indicates a failed compilation (MissingVirtualNode).
    const code = vf?.code
      ? vf.code
      : entry.source.slice(block.contentStartOffset, block.contentEndOffset);

    const langId = block.lang === "postcss" ? "css" : block.lang;
    return TextDocument.create(`${sfcUri}.style.${block.index}.${langId}`, langId, 1, code);
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
    if (existing && existing.version === version) {
      return existing;
    }

    const blocks = scanStyleBlocks(source);

    // Request virtual files from the LSP
    const virtualFiles = new Map<number, CachedStyleEntry>();
    const transpiled = new Map<number, TranspileResult>();

    try {
      const client = this.getClient();
      const response = await client.sendRequest(RequestType.GetVirtualFiles, {
        uri,
      });

      if (response?.virtualFiles) {
        for (const vf of response.virtualFiles) {
          // Parse "style:N" kind
          const match = /^style:(\d+)$/.exec(vf.kind);
          if (!match) continue;
          const idx = parseInt(match[1], 10);
          virtualFiles.set(idx, { code: vf.code, lang: vf.lang });
        }
      }
    } catch {
      // LSP might not be ready; fall back to empty
    }

    // Transpile preprocessors if needed (resolved from workspace node_modules)
    // Collect missing-preprocessor diagnostics for this URI atomically.
    const missingDiags: vscode.Diagnostic[] = [];

    for (const block of blocks) {
      if (block.lang !== "sass" && block.lang !== "stylus") continue;

      // Re-resolve if workspace path changed or wasn't available at construction
      if (this.workspacePath) {
        resolvePreprocessor(block.lang, this.workspacePath, this.preprocessors);
      }

      const vf = virtualFiles.get(block.index);
      if (!vf) continue;

      const result = await transpile(vf.code, block.lang, uri, this.preprocessors);
      if (result) {
        transpiled.set(block.index, result);

        // Send transpiled CSS back to the host for analysis
        await this.applyStyleOverride(uri, block.index, result);
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

        // Show a one-time warning message as secondary guidance
        if (!this.warnedMissing.has(block.lang)) {
          this.warnedMissing.add(block.lang);
          vscode.window.showWarningMessage(
            `Verter: "${block.lang}" is not installed in the workspace. ` +
              `CSS intellisense for <style lang="${block.lang}"> blocks requires ` +
              `"${block.lang}" to be installed as a project dependency.`,
          );
        }
      }
    }

    // Update diagnostics atomically for this URI (clears stale entries)
    try {
      const vscodeUri = vscode.Uri.parse(uri);
      this.diagnostics.set(vscodeUri, missingDiags);
    } catch {
      // URI parsing may fail for non-file URIs; ignore
    }

    const entry: DocumentCache = {
      version,
      blocks,
      source,
      virtualFiles,
      transpiled,
    };
    this.cache.set(uri, entry);
    return entry;
  }

  /**
   * Send a preprocessor-compiled style override to the Rust LSP host.
   * This updates the host's analysis with the transpiled CSS.
   */
  private async applyStyleOverride(
    uri: string,
    index: number,
    result: TranspileResult,
  ): Promise<void> {
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
      });
    } catch {
      // Silently fail — LSP might not support this yet
    }
  }
}
