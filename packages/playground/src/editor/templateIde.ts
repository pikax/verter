import type {
  FileAnalysis,
  OrderedSfcStructure,
  StructureBlock,
  StructureRange,
} from "../core/types";
import { collectCompletions } from "./analysisHelpers";
import { utf16ToUtf8Offset, utf8ToUtf16Offset } from "./offsets";

const HTML_TAGS = [
  "a",
  "article",
  "aside",
  "button",
  "canvas",
  "div",
  "footer",
  "form",
  "h1",
  "h2",
  "h3",
  "header",
  "img",
  "input",
  "label",
  "li",
  "main",
  "nav",
  "ol",
  "option",
  "p",
  "section",
  "select",
  "span",
  "svg",
  "table",
  "tbody",
  "td",
  "template",
  "textarea",
  "th",
  "thead",
  "tr",
  "ul",
] as const;

const VOID_HTML_TAGS = new Set([
  "area",
  "base",
  "br",
  "col",
  "embed",
  "hr",
  "img",
  "input",
  "link",
  "meta",
  "param",
  "source",
  "track",
  "wbr",
]);

const TEMPLATE_ATTR_COMPLETIONS: Array<{
  label: string;
  insertText: string;
  detail: string;
  isSnippet?: boolean;
}> = [
  { label: "v-if", insertText: 'v-if="$1"', detail: "Conditional render", isSnippet: true },
  {
    label: "v-else-if",
    insertText: 'v-else-if="$1"',
    detail: "Conditional branch",
    isSnippet: true,
  },
  { label: "v-else", insertText: "v-else", detail: "Fallback branch" },
  { label: "v-for", insertText: 'v-for="$1 in $2"', detail: "List render", isSnippet: true },
  { label: "v-model", insertText: 'v-model="$1"', detail: "Two-way binding", isSnippet: true },
  { label: "v-bind", insertText: 'v-bind="$1"', detail: "Object prop binding", isSnippet: true },
  { label: "v-on", insertText: 'v-on="$1"', detail: "Object event binding", isSnippet: true },
  { label: "@click", insertText: '@click="$1"', detail: "Click handler", isSnippet: true },
  { label: "@input", insertText: '@input="$1"', detail: "Input handler", isSnippet: true },
  {
    label: "@keydown",
    insertText: '@keydown="$1"',
    detail: "Keyboard handler",
    isSnippet: true,
  },
  { label: ":class", insertText: ':class="$1"', detail: "Dynamic class", isSnippet: true },
  { label: ":style", insertText: ':style="$1"', detail: "Dynamic style", isSnippet: true },
  { label: ":key", insertText: ':key="$1"', detail: "VNode key", isSnippet: true },
  { label: "ref", insertText: 'ref="$1"', detail: "Template ref", isSnippet: true },
  { label: "class", insertText: 'class="$1"', detail: "Static class", isSnippet: true },
  { label: "id", insertText: 'id="$1"', detail: "Element id", isSnippet: true },
];

const TEMPLATE_GLOBAL_COMPLETIONS: Array<{ label: string; detail: string }> = [
  { label: "$props", detail: "Component props context" },
  { label: "$attrs", detail: "Fallthrough attributes" },
  { label: "$slots", detail: "Slots object" },
  { label: "$emit", detail: "Emit function" },
  { label: "$el", detail: "Root element instance" },
  { label: "$refs", detail: "Template refs map" },
  { label: "$nextTick", detail: "Vue nextTick" },
  { label: "$watch", detail: "Component watcher" },
  { label: "$event", detail: "Current event payload (event handlers)" },
  { label: "Math", detail: "JavaScript global" },
  { label: "Date", detail: "JavaScript global" },
  { label: "Array", detail: "JavaScript global" },
  { label: "String", detail: "JavaScript global" },
  { label: "Number", detail: "JavaScript global" },
  { label: "Boolean", detail: "JavaScript global" },
  { label: "Promise", detail: "JavaScript global" },
  { label: "Map", detail: "JavaScript global" },
  { label: "Set", detail: "JavaScript global" },
  { label: "JSON", detail: "JavaScript global" },
  { label: "console", detail: "JavaScript global" },
];

export interface ImportEdit {
  offset: number;
  text: string;
}

export interface TemplateCompletion {
  label: string;
  insertText: string;
  detail: string;
  kind: "tag" | "directive" | "attribute" | "symbol";
  isSnippet?: boolean;
  sortText?: string;
  importEdit?: ImportEdit;
}

export interface TemplateCompletionParams {
  source: string;
  structure: OrderedSfcStructure | null;
  offset: number;
  activeFilename: string;
  openFilenames: string[];
  analysis: FileAnalysis | null | undefined;
}

function escapeRegExp(input: string): string {
  return input.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function sections(
  structure: OrderedSfcStructure | null,
): Extract<StructureBlock, { kind: "section" }>[] {
  return (
    structure?.blocks.filter(
      (block): block is Extract<StructureBlock, { kind: "section" }> => block.kind === "section",
    ) ?? []
  );
}

export function isOffsetInTemplateBlock(
  structure: OrderedSfcStructure | null,
  source: string,
  offset: number,
): boolean {
  // Structure ranges are UTF-8 BYTES; the editor offset is UTF-16. Convert
  // once, compare in byte space.
  const utf8Offset = utf8OffsetOf(source, offset);
  return sections(structure).some(
    (block) =>
      block.section.role.kind === "templateHost" &&
      utf8Offset >= block.section.contentRange.start &&
      utf8Offset <= block.section.contentRange.end,
  );
}

/** Bounded current-token lookback (mirrors the plugin's 256-byte contract). */
const MAX_CURRENT_TOKEN_LOOKBACK = 256;

const utf8OffsetOf = utf16ToUtf8Offset;

function isTagNameChar(char: string): boolean {
  return /[\w.-]/.test(char);
}

/**
 * Innermost stamped markup node whose OPENING span contains the offset —
 * element, recovered, or unknown nodes retaining an opening range. The
 * structure projection is the sole tag-geometry authority; raw source is
 * never scanned for `<`.
 */
function enclosingOpeningSyntax(
  structure: OrderedSfcStructure | null,
  source: string,
  offset: number,
): { openingStart: number; nameEnd: number | null; name: string | null } | null {
  if (structure === null) return null;
  const utf8Offset = utf8OffsetOf(source, offset);
  let best: { openingStart: number; nameEnd: number | null; name: string | null } | null = null;
  for (const node of structure.markupNodes) {
    const syntax = node.syntax as {
      kind: string;
      openingRange?: StructureRange;
      authoredName?: { spelling: string; range: StructureRange };
    };
    if (!["element", "recovered", "unknown"].includes(syntax.kind)) continue;
    const opening = syntax.openingRange;
    if (!opening) continue;
    if (!(utf8Offset > opening.start && utf8Offset < opening.end)) continue;
    if (best === null || opening.start > best.openingStart) {
      best = {
        openingStart: opening.start,
        nameEnd: syntax.authoredName?.range.end ?? null,
        name: syntax.authoredName?.spelling ?? null,
      };
    }
  }
  return best;
}

/**
 * Whether the offset sits inside an interpolation. Structure-first: a stamped
 * interpolation node containing the offset. Edit-time recovery for a
 * just-typed `{{` (not yet stamped): a bounded current-token lookback — the
 * candidate opener must NOT sit inside a stamped element opening span (a `{{`
 * inside an attribute value string is not an interpolation opener).
 */
export function isInsideInterpolation(
  structure: OrderedSfcStructure | null,
  source: string,
  offset: number,
): boolean {
  if (structure !== null) {
    const utf8Offset = utf8OffsetOf(source, offset);
    for (const node of structure.markupNodes) {
      const syntax = node.syntax as { kind: string; fullRange?: StructureRange };
      if (syntax.kind !== "interpolation" || !syntax.fullRange) continue;
      if (utf8Offset > syntax.fullRange.start && utf8Offset < syntax.fullRange.end) return true;
    }
  }
  const floor = Math.max(0, offset - MAX_CURRENT_TOKEN_LOOKBACK);
  const window = source.slice(floor, offset);
  const open = window.lastIndexOf("{{");
  const close = window.lastIndexOf("}}");
  if (open < 0 || open < close) return false;
  // The recovered opener is only an interpolation when it is markup content —
  // never inside a stamped element opening tag (attribute value territory).
  return enclosingOpeningSyntax(structure, source, floor + open) === null;
}

/**
 * Start of the current open-tag NAME token: the cursor's token characters
 * walked back (bounded), immediately preceded by `<`. A pure current-token
 * lex — no window delimiter search.
 */
function openTagTokenStart(source: string, offset: number): number | null {
  let start = offset;
  let budget = MAX_CURRENT_TOKEN_LOOKBACK;
  while (start > 0 && budget > 0 && isTagNameChar(source[start - 1])) {
    start -= 1;
    budget -= 1;
  }
  if (budget === 0) return null;
  if (start > 0 && source[start - 1] === "<") return start;
  return null;
}

function isTagNameContext(source: string, offset: number): boolean {
  return openTagTokenStart(source, offset) !== null;
}

/**
 * Attribute-name position: strictly inside a STAMPED opening tag, past the
 * tag-name token. The structure projection supplies the opening span; a `<`
 * appearing in text or attribute strings can never fabricate one.
 */
function isTagAttributeContext(
  structure: OrderedSfcStructure | null,
  source: string,
  offset: number,
): boolean {
  const syntax = enclosingOpeningSyntax(structure, source, offset);
  if (syntax === null || syntax.nameEnd === null) return false;
  return utf8OffsetOf(source, offset) > syntax.nameEnd;
}

/** `</name|` current-token check: token chars back, immediately preceded by `</`. */
function closingTagTokenStart(source: string, offset: number): number | null {
  let start = offset;
  let budget = MAX_CURRENT_TOKEN_LOOKBACK;
  while (start > 0 && budget > 0 && isTagNameChar(source[start - 1])) {
    start -= 1;
    budget -= 1;
  }
  if (budget === 0) return null;
  if (start > 1 && source[start - 1] === "/" && source[start - 2] === "<") return start;
  return null;
}

function isClosingTagNameContext(source: string, offset: number): boolean {
  return closingTagTokenStart(source, offset) !== null;
}

function currentTagPrefix(source: string, offset: number): string {
  const start = openTagTokenStart(source, offset);
  return start === null ? "" : source.slice(start, offset);
}

function currentClosingTagPrefix(source: string, offset: number): string {
  const start = closingTagTokenStart(source, offset);
  return start === null ? "" : source.slice(start, offset);
}

export function toPascalCase(input: string): string {
  return input
    .split(/[^a-zA-Z0-9]+/)
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join("");
}

export function relativeImportPath(fromFile: string, toFile: string): string {
  const fromParts = fromFile.split("/");
  fromParts.pop();
  const toParts = toFile.split("/");

  let i = 0;
  while (i < fromParts.length && i < toParts.length && fromParts[i] === toParts[i]) {
    i += 1;
  }

  const up = fromParts.length - i;
  const down = toParts.slice(i);
  let path = "";
  if (up > 0) path += "../".repeat(up);
  if (down.length > 0) path += down.join("/");
  if (!path.startsWith(".")) path = `./${path}`;
  return path;
}

interface ScriptBlock {
  openStart: number;
  openEnd: number;
  closeStart: number;
  content: string;
}

function findScriptBlock(
  source: string,
  structure: OrderedSfcStructure | null,
): ScriptBlock | null {
  const block = sections(structure).find((candidate) => candidate.section.role.kind === "script");
  if (!block) return null;
  // Structure ranges are UTF-8 BYTES; slicing and edit offsets are UTF-16.
  const openStart = utf8ToUtf16Offset(source, block.section.openingRange.start);
  const openEnd = utf8ToUtf16Offset(source, block.section.contentRange.start);
  const closeStart = utf8ToUtf16Offset(source, block.section.contentRange.end);
  return {
    openStart,
    openEnd,
    closeStart,
    content: source.slice(openEnd, closeStart),
  };
}

function hasComponentImport(
  source: string,
  structure: OrderedSfcStructure | null,
  componentName: string,
): boolean {
  const block = findScriptBlock(source, structure);
  if (!block) return false;

  const importRe = /import[\s\S]*?from\s+['"][^'"]+['"]\s*;?/g;
  const needle = new RegExp(`\\b${escapeRegExp(componentName)}\\b`);

  let match: RegExpExecArray | null;
  while ((match = importRe.exec(block.content)) !== null) {
    if (needle.test(match[0])) return true;
  }
  return false;
}

export function buildComponentImportEdit(
  source: string,
  structure: OrderedSfcStructure | null,
  componentName: string,
  importPath: string,
): ImportEdit | null {
  if (hasComponentImport(source, structure, componentName)) return null;

  const importStmt = `import ${componentName} from '${importPath}'`;
  const block = findScriptBlock(source, structure);

  if (!block) {
    return {
      offset: 0,
      text: `<script setup lang="ts">\n${importStmt}\n</script>\n\n`,
    };
  }

  const importRe = /import[\s\S]*?from\s+['"][^'"]+['"]\s*;?/g;
  let lastImportEnd = -1;
  let match: RegExpExecArray | null;
  while ((match = importRe.exec(block.content)) !== null) {
    lastImportEnd = match.index + match[0].length;
  }

  if (lastImportEnd >= 0) {
    return {
      offset: block.openEnd + lastImportEnd,
      text: `\n${importStmt}`,
    };
  }

  if (block.content.startsWith("\n")) {
    return {
      offset: block.openEnd + 1,
      text: `${importStmt}\n`,
    };
  }

  return {
    offset: block.openEnd,
    text: `\n${importStmt}\n`,
  };
}

function isLikelyComponentName(name: string): boolean {
  return /^[A-Z]/.test(name) || name.includes(".");
}

interface ComponentCandidate {
  name: string;
  importPath?: string;
}

function collectComponentCandidates(
  activeFilename: string,
  openFilenames: string[],
  analysis: FileAnalysis | null | undefined,
): ComponentCandidate[] {
  const out: ComponentCandidate[] = [];
  const seen = new Set<string>();

  for (const file of openFilenames) {
    if (!file.endsWith(".vue") || file === activeFilename) continue;
    const base =
      file
        .split("/")
        .pop()
        ?.replace(/\.vue$/i, "") ?? "";
    if (!base) continue;
    const name = toPascalCase(base);
    if (!name || seen.has(name)) continue;
    seen.add(name);
    out.push({
      name,
      importPath: relativeImportPath(activeFilename, file),
    });
  }

  if (analysis) {
    for (const binding of analysis.bindings) {
      if (!isLikelyComponentName(binding.name)) continue;
      if (seen.has(binding.name)) continue;
      seen.add(binding.name);
      out.push({ name: binding.name });
    }
    for (const imp of analysis.imports) {
      if (imp.isTypeOnly) continue;
      for (const b of imp.bindings) {
        if (b.isTypeOnly || !isLikelyComponentName(b.name)) continue;
        if (seen.has(b.name)) continue;
        seen.add(b.name);
        out.push({ name: b.name });
      }
    }
  }

  return out;
}

function buildTagInsertText(tag: string): string {
  if (VOID_HTML_TAGS.has(tag.toLowerCase())) {
    return `<${tag} $0 />`;
  }
  return `<${tag}>$0</${tag}>`;
}

function currentMarkupElement(
  structure: OrderedSfcStructure | null,
  source: string,
  offset: number,
): string | null {
  if (structure === null) return null;
  const utf8Offset = utf8OffsetOf(source, offset);
  let best: { start: number; name: string } | null = null;
  for (const node of structure.markupNodes) {
    const syntax = node.syntax;
    if (syntax.kind !== "element" || !("authoredName" in syntax)) continue;
    if (syntax.openingRange.end > utf8Offset || syntax.fullRange.end < utf8Offset) continue;
    if (best === null || syntax.openingRange.start > best.start) {
      best = { start: syntax.openingRange.start, name: syntax.authoredName.spelling };
    }
  }
  return best?.name ?? null;
}

/**
 * The name of the opening tag whose `>` was JUST typed at `gtIndex` — the one
 * genuine edit-time recovery in this module (the structure cannot have
 * stamped a tag whose `>` landed this keystroke). A bounded, quote-aware
 * backward lex over the current tag only: quoted attribute values are
 * skipped as units, an unquoted `>` aborts, and the walk gives up past the
 * 256-char budget. Nothing outside the just-typed tag is ever inspected.
 */
function justTypedOpenTagName(source: string, gtIndex: number): string | null {
  if (gtIndex > 0 && source[gtIndex - 1] === "/") return null; // self-closing
  let budget = MAX_CURRENT_TOKEN_LOOKBACK;
  let index = gtIndex - 1;
  while (index >= 0 && budget > 0) {
    const char = source[index];
    if (char === '"' || char === "'") {
      const openQuote = source.lastIndexOf(char, index - 1);
      if (openQuote < 0 || gtIndex - openQuote > MAX_CURRENT_TOKEN_LOOKBACK) return null;
      budget -= index - openQuote;
      index = openQuote - 1;
      continue;
    }
    if (char === ">") return null;
    if (char === "<") {
      if (source[index + 1] === "/") return null; // closing tag
      const match = /^<([A-Za-z][\w.-]*)\b/.exec(source.slice(index, gtIndex + 1));
      return match ? match[1] : null;
    }
    index -= 1;
    budget -= 1;
  }
  return null;
}

export function computeAutoCloseTagText(
  source: string,
  structure: OrderedSfcStructure | null,
  offset: number,
): string | null {
  if (offset <= 0 || source[offset - 1] !== ">") return null;
  if (
    !isOffsetInTemplateBlock(structure, source, offset) ||
    isInsideInterpolation(structure, source, offset)
  ) {
    return null;
  }

  const tagName = justTypedOpenTagName(source, offset - 1);
  if (tagName === null) return null;
  if (VOID_HTML_TAGS.has(tagName.toLowerCase())) return null;

  const after = source.slice(offset).trimStart();
  if (after.startsWith(`</${tagName}`)) return null;

  return `</${tagName}>`;
}

function collectClosingTagCompletions(
  source: string,
  structure: OrderedSfcStructure | null,
  offset: number,
): TemplateCompletion[] {
  if (!isClosingTagNameContext(source, offset)) return [];
  // A `</` DECOY inside a stamped opening tag (an attribute value string) is
  // not a closing tag — the structure knows the cursor is tag-internal.
  if (enclosingOpeningSyntax(structure, source, offset) !== null) return [];

  const expectedTag = currentMarkupElement(structure, source, offset);
  if (!expectedTag) return [];

  const prefix = currentClosingTagPrefix(source, offset).toLowerCase();
  if (prefix && !expectedTag.toLowerCase().startsWith(prefix)) return [];

  return [
    {
      label: expectedTag,
      insertText: `${expectedTag}>`,
      detail: "Close current element",
      kind: "tag",
      sortText: `0_${expectedTag}`,
    },
  ];
}

export function collectTemplateInterpolationCompletions(
  params: TemplateCompletionParams,
): TemplateCompletion[] {
  const { source, offset, analysis } = params;
  if (
    !isOffsetInTemplateBlock(params.structure, source, offset) ||
    !isInsideInterpolation(params.structure, source, offset)
  ) {
    return [];
  }

  const out: TemplateCompletion[] = [];
  const seen = new Set<string>();

  if (analysis) {
    for (const entry of collectCompletions(analysis, false)) {
      if (seen.has(entry.label)) continue;
      seen.add(entry.label);
      out.push({
        label: entry.label,
        insertText: entry.label,
        detail: entry.detail,
        kind: "symbol",
        sortText: `0_${entry.label}`,
      });
    }
  }

  for (const global of TEMPLATE_GLOBAL_COMPLETIONS) {
    if (seen.has(global.label)) continue;
    seen.add(global.label);
    out.push({
      label: global.label,
      insertText: global.label,
      detail: global.detail,
      kind: "symbol",
      sortText: `1_${global.label}`,
    });
  }

  return out;
}

export function collectTemplateCompletions(params: TemplateCompletionParams): TemplateCompletion[] {
  const { source, offset, activeFilename, openFilenames, analysis } = params;

  if (
    !isOffsetInTemplateBlock(params.structure, source, offset) ||
    isInsideInterpolation(params.structure, source, offset)
  ) {
    return [];
  }

  const closing = collectClosingTagCompletions(source, params.structure, offset);
  if (closing.length > 0) return closing;

  if (isTagNameContext(source, offset)) {
    const prefix = currentTagPrefix(source, offset).toLowerCase();
    const out: TemplateCompletion[] = [];

    for (const tag of HTML_TAGS) {
      if (prefix && !tag.startsWith(prefix)) continue;
      out.push({
        label: tag,
        kind: "tag",
        detail: "HTML element",
        insertText: buildTagInsertText(tag),
        isSnippet: true,
        sortText: `2_${tag}`,
      });
    }

    for (const component of collectComponentCandidates(activeFilename, openFilenames, analysis)) {
      if (prefix && !component.name.toLowerCase().startsWith(prefix)) continue;
      const importEdit = component.importPath
        ? buildComponentImportEdit(source, params.structure, component.name, component.importPath)
        : null;
      out.push({
        label: component.name,
        kind: "tag",
        detail: importEdit ? `Component (auto-import ${component.importPath})` : "Component",
        insertText: `<${component.name} $0 />`,
        isSnippet: true,
        sortText: `1_${component.name}`,
        importEdit: importEdit ?? undefined,
      });
    }

    return out;
  }

  if (isTagAttributeContext(params.structure, source, offset)) {
    return TEMPLATE_ATTR_COMPLETIONS.map((item) => ({
      label: item.label,
      insertText: item.insertText,
      detail: item.detail,
      kind:
        item.label.startsWith("v-") || item.label.startsWith("@") || item.label.startsWith(":")
          ? "directive"
          : "attribute",
      isSnippet: item.isSnippet ?? false,
      sortText: `3_${item.label}`,
    }));
  }

  return [];
}
