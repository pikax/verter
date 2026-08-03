/**
 * Pure render decisions for the `$/verter/getBindingTypes` wire — the display
 * helpers BOTH tree providers call (AnalysisTreeProvider, ComponentTreeProvider).
 *
 * `displaySignature` is the TypeProvider's quick-info display string VERBATIM
 * (e.g. `const count: Ref<number>`). It is display-only: these helpers render
 * it as-is and NEVER split it on `": "`, slice it, match it against the
 * binding name, or otherwise recover structure from it — any such re-split
 * re-creates the deleted markdown-scraper class inside JavaScript and is a
 * Native-vs-Compat violation ("JS may transform structure but must not
 * recover meaning").
 *
 * VS-Code-API-free by design so the vitest lane (`vitest run --dir src`) can
 * exercise the exact functions production renders through.
 */

/** One wire entry: the provider's display signature, or `null` (unavailable /
 * superseded surface — fail closed). */
export type BindingTypeEntry = { displaySignature: string } | null;

/** The signature to render, or `null` when the entry is absent/failed-closed. */
export function bindingSignature(entry: BindingTypeEntry | undefined): string | null {
  return entry?.displaySignature ?? null;
}

/** Leaf `description` for the Type Information tree: the signature verbatim,
 * empty when absent (never the strings "null"/"undefined"). */
export function bindingLeafDescription(entry: BindingTypeEntry | undefined): string {
  return bindingSignature(entry) ?? "";
}

/** Leaf `tooltip` for the Type Information tree: the signature VERBATIM — the
 * signature already names the binding, so no `${name}: ` prefix is ever
 * prepended (that duplication was the deleted defect). Falls back to the bare
 * binding name when the entry is absent. */
export function bindingLeafTooltip(name: string, entry: BindingTypeEntry | undefined): string {
  return bindingSignature(entry) ?? name;
}

/** One tooltip line for a binding leaf: `Signature: <signature>` (the value is
 * a display signature, not a type — the label says what it is). Empty when
 * absent so `.filter(Boolean)` composition drops it. */
export function bindingTooltipLine(entry: BindingTypeEntry | undefined): string {
  return signatureTooltipLine(bindingSignature(entry));
}

/** The one `Signature: …` tooltip-line spelling, shared by every tree
 * surface; empty when no signature exists. */
export function signatureTooltipLine(signature: string | null): string {
  return signature ? `Signature: ${signature}` : "";
}

/** Component-tree prop `description`: `(<signature>, <constness>)` from the
 * same helper surface, omitting absent parts. Empty when neither exists. */
export function propSignatureDescription(signature: string | null, constness: string): string {
  const parts = [signature, constness].filter(Boolean);
  return parts.length > 0 ? `(${parts.join(", ")})` : "";
}
