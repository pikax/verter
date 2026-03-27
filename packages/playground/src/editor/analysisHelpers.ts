/**
 * Pure helper functions for hover and completion logic using Verter analysis data.
 * No Monaco dependency — can be unit-tested directly.
 */
import type {
  FileAnalysis,
  AnalysisBinding,
  AnalysisImportBinding,
  AnalysisMacro,
  AnalysisBindingInitializer,
} from "../core/types";

// ── Hover formatting ──

export function formatBindingHover(binding: AnalysisBinding): string {
  const lines: string[] = [];

  const kindStr = binding.kind === "AsyncFunction" ? "async function" : binding.kind.toLowerCase();
  const typeStr = binding.typeAnnotation ? `: ${binding.typeAnnotation}` : "";
  lines.push(`\`\`\`typescript\n${kindStr} ${binding.name}${typeStr}\n\`\`\``);

  if (binding.reactivityKind) {
    switch (binding.reactivityKind) {
      case "Ref":
        lines.push("*(ref — needs `.value`)*");
        break;
      case "Computed":
        lines.push("*(computed — needs `.value`, read-only)*");
        break;
      case "Reactive":
        lines.push("*(reactive — direct property access)*");
        break;
      case "MaybeRef":
        lines.push("*(maybe ref — may need `.value`)*");
        break;
      case "Mutable":
        lines.push("*(mutable — reassignable)*");
        break;
      default:
        if (binding.isReactive) lines.push("*(reactive)*");
    }
  } else if (binding.isReactive) {
    lines.push("*(reactive)*");
  }

  if (binding.initializer && binding.initializer !== "Other") {
    const init = binding.initializer as Exclude<AnalysisBindingInitializer, "Other">;
    if ("FunctionCall" in init) {
      const { callee, calleeImportSource } = init.FunctionCall;
      const sourceInfo = calleeImportSource ? ` (from \`${calleeImportSource}\`)` : "";
      lines.push(`Initialized via \`${callee}()\`${sourceInfo}`);
    } else if ("Literal" in init) {
      lines.push(`Literal: ${init.Literal.kind}`);
    } else if ("Reference" in init) {
      lines.push(`References \`${init.Reference.name}\``);
    }
  }

  return lines.join("\n\n");
}

export function formatImportHover(binding: AnalysisImportBinding, source: string): string {
  const typePrefix = binding.isTypeOnly ? "type " : "";
  const lines = [
    `\`\`\`typescript\nimport ${typePrefix}{ ${binding.name} } from '${source}'\n\`\`\``,
  ];

  if (binding.vueApi) {
    lines.push(`Vue API: \`${binding.vueApi}\``);
  }

  return lines.join("\n\n");
}

export function formatMacroHover(mac: AnalysisMacro): string {
  const macroName = mac.kind;
  const lines: string[] = [];

  if (mac.bindingName) {
    lines.push(`\`\`\`typescript\nconst ${mac.bindingName} = ${macroName}()\n\`\`\``);
  } else {
    lines.push(`\`\`\`typescript\n${macroName}()\n\`\`\``);
  }

  if (mac.isTypeBased) {
    const types = mac.typeReferences.length === 0 ? "inline type" : mac.typeReferences.join(", ");
    lines.push(`Type-based: \`<${types}>\``);
  }

  return lines.join("\n\n");
}

export function hoverForWord(word: string, analysis: FileAnalysis): string | null {
  const binding = analysis.bindings.find((b) => b.name === word);
  if (binding) return formatBindingHover(binding);

  for (const imp of analysis.imports) {
    const impBinding = imp.bindings.find((b) => b.name === word);
    if (impBinding) return formatImportHover(impBinding, imp.source);
  }

  for (const mac of analysis.macros) {
    if (mac.bindingName === word) return formatMacroHover(mac);
  }

  return null;
}

// ── Completion helpers ──

export interface CompletionEntry {
  label: string;
  kind: string; // "Constant" | "Variable" | "Function" | "Class" | "Module" | "TypeParameter"
  detail: string;
}

function bindingDetail(binding: AnalysisBinding): string {
  const base = binding.kind === "AsyncFunction" ? "async function" : binding.kind.toLowerCase();
  if (binding.reactivityKind && binding.reactivityKind !== "None") {
    return `${base} (${binding.reactivityKind.toLowerCase()})`;
  }
  if (binding.isReactive) return `${base} (reactive)`;
  return base;
}

function bindingCompletionKind(kind: string): string {
  switch (kind) {
    case "Const":
      return "Constant";
    case "Let":
    case "Var":
      return "Variable";
    case "Function":
    case "AsyncFunction":
      return "Function";
    case "Class":
      return "Class";
    default:
      return "Variable";
  }
}

export function collectCompletions(
  analysis: FileAnalysis,
  includeTypeOnlyImports: boolean,
): CompletionEntry[] {
  const items: CompletionEntry[] = [];
  const seen = new Set<string>();

  for (const binding of analysis.bindings) {
    if (binding.name.startsWith("___VERTER___")) continue;
    if (seen.has(binding.name)) continue;
    seen.add(binding.name);
    items.push({
      label: binding.name,
      kind: bindingCompletionKind(binding.kind),
      detail: bindingDetail(binding),
    });
  }

  for (const imp of analysis.imports) {
    if (!includeTypeOnlyImports && imp.isTypeOnly) continue;
    for (const binding of imp.bindings) {
      if (!includeTypeOnlyImports && binding.isTypeOnly) continue;
      if (binding.name.startsWith("___VERTER___")) continue;
      if (seen.has(binding.name)) continue;
      seen.add(binding.name);
      items.push({
        label: binding.name,
        kind: binding.isTypeOnly || imp.isTypeOnly ? "TypeParameter" : "Module",
        detail: `from '${imp.source}'`,
      });
    }
  }

  for (const mac of analysis.macros) {
    if (!mac.bindingName) continue;
    if (seen.has(mac.bindingName)) continue;
    seen.add(mac.bindingName);
    items.push({
      label: mac.bindingName,
      kind: "Variable",
      detail: `${mac.kind}()`,
    });
  }

  return items;
}

/** Simple check: is the offset within a <script> block? */
export function isOffsetInScriptBlock(source: string, offset: number): boolean {
  const scriptOpenRe = /<script[^>]*>/gi;
  let match;
  while ((match = scriptOpenRe.exec(source)) !== null) {
    const openEnd = match.index + match[0].length;
    const closeIdx = source.indexOf("</script>", openEnd);
    if (closeIdx === -1) continue;
    if (offset >= openEnd && offset <= closeIdx) return true;
  }
  return false;
}
