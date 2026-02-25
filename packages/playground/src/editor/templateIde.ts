import type { FileAnalysis } from "../core/types";
import { collectCompletions } from "./analysisHelpers";

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
  offset: number;
  activeFilename: string;
  openFilenames: string[];
  analysis: FileAnalysis | null | undefined;
}

function escapeRegExp(input: string): string {
  return input.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function isOffsetInTemplateBlock(source: string, offset: number): boolean {
  const templateOpen = source.lastIndexOf("<template", offset);
  if (templateOpen === -1) return false;

  const openEnd = source.indexOf(">", templateOpen);
  if (openEnd === -1 || offset < openEnd + 1) return false;

  const templateClose = source.indexOf("</template>", openEnd + 1);
  if (templateClose === -1) return true;
  return offset <= templateClose;
}

export function isInsideInterpolation(source: string, offset: number): boolean {
  const open = source.lastIndexOf("{{", offset);
  const close = source.lastIndexOf("}}", offset);
  return open > close;
}

function getOpenTagStart(source: string, offset: number): number | null {
  const searchOffset = Math.max(0, offset - 1);
  const lt = source.lastIndexOf("<", searchOffset);
  const gt = source.lastIndexOf(">", searchOffset);
  if (lt === -1 || lt < gt) return null;

  const marker = source[lt + 1];
  if (marker === "/" || marker === "!" || marker === "?") return null;
  return lt;
}

function isTagNameContext(source: string, offset: number): boolean {
  const openTagStart = getOpenTagStart(source, offset);
  if (openTagStart == null) return false;

  const between = source.slice(openTagStart + 1, offset);
  return !/\s/.test(between);
}

function isTagAttributeContext(source: string, offset: number): boolean {
  const openTagStart = getOpenTagStart(source, offset);
  if (openTagStart == null) return false;
  const between = source.slice(openTagStart + 1, offset);
  if (between.length === 0) return false;
  return /\s/.test(between);
}

function isClosingTagNameContext(source: string, offset: number): boolean {
  const searchOffset = Math.max(0, offset - 1);
  const lt = source.lastIndexOf("<", searchOffset);
  const gt = source.lastIndexOf(">", searchOffset);
  if (lt === -1 || lt < gt) return false;

  if (source.slice(lt, lt + 2) !== "</") return false;
  const between = source.slice(lt + 2, offset);
  return !/\s/.test(between);
}

function currentTagPrefix(source: string, offset: number): string {
  const openTagStart = getOpenTagStart(source, offset);
  if (openTagStart == null) return "";
  const raw = source.slice(openTagStart + 1, offset);
  return raw.trim();
}

function currentClosingTagPrefix(source: string, offset: number): string {
  const searchOffset = Math.max(0, offset - 1);
  const lt = source.lastIndexOf("<", searchOffset);
  if (lt === -1) return "";
  return source.slice(lt + 2, offset).trim();
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

function findScriptBlock(source: string): ScriptBlock | null {
  const setupMatch = /<script\b[^>]*\bsetup\b[^>]*>/i.exec(source);
  const anyMatch = /<script\b[^>]*>/i.exec(source);
  const match = setupMatch ?? anyMatch;
  if (!match || match.index == null) return null;

  const openStart = match.index;
  const openEnd = openStart + match[0].length;
  const closeStart = source.indexOf("</script>", openEnd);
  if (closeStart === -1) return null;

  return {
    openStart,
    openEnd,
    closeStart,
    content: source.slice(openEnd, closeStart),
  };
}

function hasComponentImport(source: string, componentName: string): boolean {
  const block = findScriptBlock(source);
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
  componentName: string,
  importPath: string,
): ImportEdit | null {
  if (hasComponentImport(source, componentName)) return null;

  const importStmt = `import ${componentName} from '${importPath}'`;
  const block = findScriptBlock(source);

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
    const base = file.split("/").pop()?.replace(/\.vue$/i, "") ?? "";
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

function findLastUnclosedTag(source: string, offset: number): string | null {
  const before = source.slice(0, offset);
  const tagRe = /<\/?([A-Za-z][\w.-]*)\b[^>]*>/g;
  const stack: string[] = [];

  let match: RegExpExecArray | null;
  while ((match = tagRe.exec(before)) !== null) {
    const full = match[0];
    const name = match[1];
    const lower = name.toLowerCase();

    if (full.startsWith("</")) {
      const last = stack[stack.length - 1];
      if (last && last.toLowerCase() === lower) {
        stack.pop();
      }
      continue;
    }

    if (full.endsWith("/>") || VOID_HTML_TAGS.has(lower)) {
      continue;
    }

    stack.push(name);
  }

  return stack[stack.length - 1] ?? null;
}

export function computeAutoCloseTagText(source: string, offset: number): string | null {
  if (offset <= 0 || source[offset - 1] !== ">") return null;
  if (!isOffsetInTemplateBlock(source, offset) || isInsideInterpolation(source, offset)) {
    return null;
  }

  const openTagStart = getOpenTagStart(source, offset - 1);
  if (openTagStart == null) return null;

  const openTagText = source.slice(openTagStart, offset);
  if (openTagText.startsWith("</") || openTagText.endsWith("/>")) return null;

  const match = /^<([A-Za-z][\w.-]*)\b/.exec(openTagText);
  if (!match) return null;

  const tagName = match[1];
  if (VOID_HTML_TAGS.has(tagName.toLowerCase())) return null;

  const after = source.slice(offset).trimStart();
  if (after.startsWith(`</${tagName}`)) return null;

  return `</${tagName}>`;
}

function collectClosingTagCompletions(source: string, offset: number): TemplateCompletion[] {
  if (!isClosingTagNameContext(source, offset)) return [];

  const expectedTag = findLastUnclosedTag(source, offset);
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
  if (!isOffsetInTemplateBlock(source, offset) || !isInsideInterpolation(source, offset)) {
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

  if (!isOffsetInTemplateBlock(source, offset) || isInsideInterpolation(source, offset)) {
    return [];
  }

  const closing = collectClosingTagCompletions(source, offset);
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
        ? buildComponentImportEdit(source, component.name, component.importPath)
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

  if (isTagAttributeContext(source, offset)) {
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
