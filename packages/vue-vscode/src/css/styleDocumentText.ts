/**
 * The text a position-bearing CSS language service must parse for a DIRECT
 * (css/scss/less/postcss) style block: the VERBATIM authored slice of the SFC
 * source.
 *
 * The position mapping between the virtual CSS document and the SFC
 * (`toCssPosition` / `toSfcPosition` in cssService) is pure line/column
 * arithmetic anchored at the block's content start. That arithmetic is valid
 * ONLY when the CSS document's text is byte-identical to the authored slice.
 * Feeding the COMPILED style output (scoped `[data-v-*]` selector rewrites,
 * `v-bind()` → `var(--…)` rewrites, leading-trim) shifts lines/columns and
 * mis-maps every returned range — the observed defect was a color chip
 * rendering ON the first class name of `<style>` instead of on the color
 * value.
 */
export function directStyleDocumentText(
  source: string,
  block: { contentStartOffset: number; contentEndOffset: number },
): string {
  return source.slice(block.contentStartOffset, block.contentEndOffset);
}
