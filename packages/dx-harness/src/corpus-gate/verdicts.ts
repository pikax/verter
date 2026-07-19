/**
 * Result-shape classification for corpus-gate responses (pure).
 *
 * Each classifier answers one question: did the response carry content?
 * `empty` is a structural observation (null / blank hover / zero locations /
 * zero items) — whether an empty is a FAILURE is decided later against the
 * config's allowed-empty categories.
 */

/** Whether a hover response carries any text content. */
export function hoverIsEmpty(result: unknown): boolean {
  if (result == null) return true;
  const contents = (result as { contents?: unknown }).contents;
  const text =
    typeof contents === "string"
      ? contents
      : Array.isArray(contents)
        ? contents
            .map((entry) =>
              typeof entry === "string"
                ? entry
                : String((entry as { value?: unknown })?.value ?? ""),
            )
            .join("\n")
        : String((contents as { value?: unknown } | null | undefined)?.value ?? "");
  return text.trim().length === 0;
}

/** Whether a definition/declaration response carries any location. */
export function definitionIsEmpty(result: unknown): boolean {
  if (result == null) return true;
  const locations = Array.isArray(result) ? result : [result];
  return locations.length === 0;
}

/** Whether a completion response carries any item. */
export function completionIsEmpty(result: unknown): boolean {
  if (result == null) return true;
  const items = Array.isArray(result)
    ? result
    : (((result as { items?: unknown }).items as unknown[] | undefined) ?? []);
  return items.length === 0;
}

/** Whether a references response carries any location. */
export function referencesIsEmpty(result: unknown): boolean {
  if (result == null) return true;
  return !Array.isArray(result) || result.length === 0;
}
