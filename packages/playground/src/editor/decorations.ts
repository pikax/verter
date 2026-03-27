/**
 * Pure functions computing Monaco decoration arrays from Verter analysis data.
 * No Monaco dependency — returns plain objects that Editor.vue maps to Monaco types.
 */
import type { FileAnalysis, AnalysisBinding } from "../core/types";

/** Decoration descriptor for a binding in the script block. */
export interface BindingDecoration {
  /** UTF-16 start offset in the SFC source. */
  start: number;
  /** UTF-16 end offset in the SFC source. */
  end: number;
  /** CSS class name for the inline decoration. */
  className: string;
  /** Hover message (markdown). */
  hoverMessage: string;
}

/** Decoration descriptor for a CSS class usage status. */
export interface CssClassDecoration {
  /** UTF-16 start offset in the SFC source. */
  start: number;
  /** UTF-16 end offset in the SFC source. */
  end: number;
  /** Whether the class is used in the template. */
  used: boolean;
  /** CSS class name for the inline decoration. */
  className: string;
}

/** CodeLens descriptor for a script/template/style block. */
export interface BlockCodeLens {
  /** Line number (1-based) where the code lens should appear. */
  line: number;
  /** Display text for the code lens. */
  title: string;
}

// ── Reactivity color mapping ──

const REACTIVITY_COLORS: Record<string, { className: string; label: string }> = {
  Ref: { className: "verter-ref", label: "ref" },
  Computed: { className: "verter-computed", label: "computed" },
  Reactive: { className: "verter-reactive", label: "reactive" },
  MaybeRef: { className: "verter-ref", label: "maybe-ref" },
  Mutable: { className: "verter-mutable", label: "mutable" },
};

const KIND_COLORS: Record<string, { className: string; label: string }> = {
  Function: { className: "verter-function", label: "function" },
  AsyncFunction: { className: "verter-function", label: "async function" },
  Class: { className: "verter-class", label: "class" },
};

function getBindingStyle(binding: AnalysisBinding): { className: string; label: string } | null {
  if (binding.reactivityKind && binding.reactivityKind !== "None") {
    return REACTIVITY_COLORS[binding.reactivityKind] ?? null;
  }
  if (binding.isReactive) {
    return { className: "verter-reactive", label: "reactive" };
  }
  return KIND_COLORS[binding.kind] ?? null;
}

/**
 * Compute binding reactivity decorations from analysis data.
 * Returns decorations for each binding declaration AND template binding occurrences.
 *
 * Uses SFC-absolute spans from the analysis for precise positioning.
 */
export function computeBindingDecorations(
  _source: string,
  analysis: FileAnalysis,
): BindingDecoration[] {
  const decorations: BindingDecoration[] = [];

  // Build a lookup map of binding name → style
  const bindingStyles = new Map<string, { className: string; label: string }>();

  // Script binding declarations (using span data)
  for (const binding of analysis.bindings) {
    if (binding.name.startsWith("___VERTER___")) continue;
    const style = getBindingStyle(binding);
    if (!style) continue;

    bindingStyles.set(binding.name, style);

    // Use SFC-absolute span from analysis
    if (binding.spanStart > 0 || binding.spanEnd > 0) {
      decorations.push({
        start: binding.spanStart,
        end: binding.spanEnd,
        className: style.className,
        hoverMessage: `**${style.label}** binding`,
      });
    }
  }

  // Template binding occurrences (all usages in the template)
  if (analysis.template?.bindingOccurrences) {
    for (const occ of analysis.template.bindingOccurrences) {
      const style = bindingStyles.get(occ.name);
      if (!style) continue;

      decorations.push({
        start: occ.spanStart,
        end: occ.spanEnd,
        className: style.className,
        hoverMessage: `**${style.label}** binding (${occ.usageKind})`,
      });
    }
  }

  return decorations;
}

/**
 * Compute CSS class usage decorations.
 * Marks classes defined in <style> blocks as used/unused based on template analysis.
 */
export function computeCssClassDecorations(analysis: FileAnalysis): CssClassDecoration[] {
  const decorations: CssClassDecoration[] = [];

  // Collect all template class names (from static classes on elements)
  // Analysis doesn't have template element data exposed to JS yet,
  // so we use the CSS analysis to show all defined classes
  const allDefinedClasses = new Set<string>();

  for (const style of analysis.styles) {
    if (!style.css) continue;
    for (const cls of style.css.classes) {
      // Track unique class names
      allDefinedClasses.add(cls.name);

      decorations.push({
        start: cls.start,
        end: cls.end,
        used: true, // Default to true since we don't have template usage data yet
        className: "verter-css-used",
      });
    }
  }

  return decorations;
}

/**
 * Compute CodeLens summaries for SFC blocks.
 * Returns one code lens per block with a summary of its contents.
 */
export function computeCodeLenses(source: string, analysis: FileAnalysis): BlockCodeLens[] {
  const lenses: BlockCodeLens[] = [];

  // Script setup block summary
  const scriptMatch = /<script[^>]*\bsetup\b[^>]*>/i.exec(source);
  if (scriptMatch) {
    const line = countLines(source, scriptMatch.index);
    const parts: string[] = [];

    const bindingCount = analysis.bindings.filter((b) => !b.name.startsWith("___VERTER___")).length;
    if (bindingCount > 0) parts.push(`${bindingCount} binding${bindingCount !== 1 ? "s" : ""}`);

    const importCount = analysis.imports.length;
    if (importCount > 0) parts.push(`${importCount} import${importCount !== 1 ? "s" : ""}`);

    const macroCount = analysis.macros.length;
    if (macroCount > 0) parts.push(`${macroCount} macro${macroCount !== 1 ? "s" : ""}`);

    // Props summary from defineProps
    const propsMacro = analysis.macros.find((m) => m.kind === "defineProps");
    if (propsMacro) {
      const typeBased = propsMacro.isTypeBased ? " (type-based)" : "";
      parts.push(`defineProps${typeBased}`);
    }

    if (parts.length > 0) {
      lenses.push({ line, title: parts.join(" · ") });
    }
  }

  // Template block summary
  const templateMatch = /<template\b[^>]*>/i.exec(source);
  if (templateMatch) {
    const line = countLines(source, templateMatch.index);
    // We don't have template element count from analysis yet
    lenses.push({ line, title: "template" });
  }

  // Style block summaries
  const styleRegex = /<style\b([^>]*)>/gi;
  let styleMatch;
  let styleIdx = 0;
  while ((styleMatch = styleRegex.exec(source)) !== null) {
    const line = countLines(source, styleMatch.index);
    const styleAnalysis = analysis.styles[styleIdx];
    const parts: string[] = [];

    if (styleAnalysis?.scoped) parts.push("scoped");
    if (styleAnalysis?.isModule) parts.push("module");
    if (styleAnalysis?.css) {
      const classCount = styleAnalysis.css.classes.length;
      if (classCount > 0) parts.push(`${classCount} class${classCount !== 1 ? "es" : ""}`);
      const ruleCount = styleAnalysis.css.ruleCount;
      if (ruleCount > 0) parts.push(`${ruleCount} rule${ruleCount !== 1 ? "s" : ""}`);
    }
    if (styleAnalysis?.vBinds.length) {
      parts.push(`${styleAnalysis.vBinds.length} v-bind`);
    }

    if (parts.length > 0) {
      lenses.push({ line, title: parts.join(" · ") });
    }

    styleIdx++;
  }

  return lenses;
}

// ── Inlay hints ──

/** Inlay hint descriptor for Monaco. */
export interface BindingInlayHint {
  /** SFC-absolute byte offset where the hint should appear. */
  position: number;
  /** Display text for the hint. */
  label: string;
  /** Hint kind: "type" or "parameter". */
  kind: "type" | "parameter";
}

/**
 * Compute per-binding type inlay hints from analysis data.
 * Returns hints for reactive bindings without explicit type annotations.
 */
export function computeBindingInlayHints(analysis: FileAnalysis): BindingInlayHint[] {
  const hints: BindingInlayHint[] = [];

  for (const binding of analysis.bindings) {
    if (binding.name.startsWith("___VERTER___")) continue;
    if (binding.typeAnnotation) continue;
    if (!binding.reactivityKind || binding.reactivityKind === "None") continue;

    const typeText = inferTypeText(binding);
    if (!typeText) continue;

    hints.push({
      position: binding.spanEnd,
      label: `: ${typeText}`,
      kind: "type",
    });
  }

  return hints;
}

function inferTypeText(binding: AnalysisBinding): string | null {
  const kind = binding.reactivityKind;
  if (!kind) return null;

  if (kind === "Ref") {
    if (
      binding.initializer &&
      "FunctionCall" in binding.initializer &&
      binding.initializer.FunctionCall.vueApi === "Computed"
    ) {
      return "ComputedRef<...>";
    }
    return "Ref<...>";
  }
  if (kind === "Computed") return "ComputedRef<...>";
  if (kind === "Reactive") return "reactive object";
  if (kind === "MaybeRef") {
    if (binding.initializer && "FunctionCall" in binding.initializer) {
      return `ReturnType<typeof ${binding.initializer.FunctionCall.callee}>`;
    }
    return "MaybeRef<...>";
  }
  return null;
}

// ── CSS for Monaco decorations ──

/** Returns CSS rules to inject into the page for binding decorations. */
export function getDecorationStyles(): string {
  return `
    .verter-ref { color: #3b82f6 !important; text-decoration: underline; text-decoration-color: rgba(59, 130, 246, 0.4); }
    .verter-computed { color: #8b5cf6 !important; text-decoration: underline; text-decoration-color: rgba(139, 92, 246, 0.4); }
    .verter-reactive { color: #06b6d4 !important; text-decoration: underline; text-decoration-color: rgba(6, 182, 212, 0.4); }
    .verter-mutable { color: #eab308 !important; text-decoration: underline; text-decoration-color: rgba(234, 179, 8, 0.4); }
    .verter-function { color: #22c55e !important; }
    .verter-class { color: #f97316 !important; }
    .verter-css-used { background: rgba(34, 197, 94, 0.08); }
    .verter-css-unused { text-decoration: line-through; opacity: 0.5; }
  `;
}

// ── Helpers ──

/** Count lines (1-based) up to a byte offset. */
function countLines(source: string, offset: number): number {
  let count = 1;
  for (let i = 0; i < offset && i < source.length; i++) {
    if (source.charCodeAt(i) === 10) count++;
  }
  return count;
}
